use chrono::{DateTime, Utc};
use clap::Parser;
use diameter::SpanningTree;
use irctokens::Line;
use std::{
    collections::BTreeMap,
    fmt::Write,
    time::{Instant, SystemTime},
};
use tokio::{
    io::{self, AsyncBufReadExt, AsyncWriteExt, BufReader, ReadHalf, WriteHalf},
    net::TcpStream,
    sync::Mutex,
};

macro_rules! utf8ify {
    ($var:tt) => {
        let $var = std::str::from_utf8($var.as_ref()).map_err(BotError::Utf8Error)?;
    };
    ($var:tt, $($tail:tt)*) => {
        utf8ify!($var);
        utf8ify!($($tail)*);
    };
}

#[derive(Debug, Parser)]
struct Opt {
    #[arg(required = true)]
    addr: String,
}

struct Bot {
    read: Mutex<BufReader<ReadHalf<TcpStream>>>,
    write: Mutex<WriteHalf<TcpStream>>,
    links: Mutex<SpanningTree>,
    rlinks: Mutex<BTreeMap<usize, Vec<usize>>>,
    uptimes: Mutex<BTreeMap<(usize, usize), usize>>,
    received: Mutex<usize>,
    started: Mutex<Option<Instant>>,
}

impl Bot {
    async fn connect(addr: &str, nick: &str) -> io::Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let (read, mut write) = io::split(stream);
        let read = Mutex::new(BufReader::new(read));
        write
            .write_all(format!("NICK {}\r\nUSER linkuptime 0 * linkuptime\r\n", nick).as_bytes())
            .await?;
        let write = Mutex::new(write);
        Ok(Bot {
            read,
            write,
            links: Mutex::new(SpanningTree::default()),
            rlinks: Mutex::new(BTreeMap::new()),
            uptimes: Mutex::new(BTreeMap::new()),
            received: Mutex::new(0),
            started: Mutex::new(None),
        })
    }
    async fn write_line(&self, line: &Line) -> Result<(), BotError> {
        let mut writer = self.write.lock().await;
        // Line::write_to is not async friendly :(
        //line.write_to(&mut writer).map_err(BotError::IoError)?;
        let mut output = line.format();
        output.push(b'\r');
        output.push(b'\n');
        writer.write_all(&output).await.map_err(BotError::IoError)
    }
    async fn begin(&self) -> Result<(), BotError> {
        let mut started = self.started.lock().await;
        *started = Some(Instant::now());
        self.write_line(&Line {
            tags: None,
            source: None,
            command: "LINKS".to_string(),
            arguments: vec![],
        })
        .await
    }
    async fn finish(&self) -> String {
        let mut out = r#"graph L {
bgcolor = black;
node [color=white;fontcolor=white;fontname="Comic Sans MS"];
edge [penwidth=2;color=white;fontcolor=white;fontname="Comic Sans MS"];
"#
        .to_string();
        let links = self.links.lock().await;

        {
            let rlinks = self.rlinks.lock().await;
            let uptimes = self.uptimes.lock().await;
            let (_, names) = links.tree();
            for (rid, peers) in rlinks.iter() {
                let right = &names[*rid];
                for lid in peers {
                    let left = &names[*lid];
                    if let Some(uptime) = uptimes.get(&(*rid, *lid)) {
                        let (up, hue) = display(*uptime);
                        writeln!(
                            out,
                            r#""{right}" -- "{left}" [label="{up}";color="{hue},1,.8"]"#
                        )
                        .unwrap();
                    } else {
                        writeln!(out, r#""{right}" -- "{left}""#).unwrap();
                    }
                }
            }
        }

        if let Some(d) = links.diameter() {
            if let Some(start) = *self.started.lock().await {
                writeln!(
                    out,
                    r#""longest path {} hops, from {} to {}\n{} seconds elapsed, {}" [shape=box];"#,
                    d.0,
                    d.1,
                    d.2,
                    start.elapsed().as_secs(),
                    time_now(),
                )
                .unwrap();
            }
        }
        out.push('}');
        out
    }
    async fn run(&self) -> Result<(), BotError> {
        let mut buf = Vec::with_capacity(512);
        loop {
            let length = self
                .read
                .lock()
                .await
                .read_until(b'\n', &mut buf)
                .await
                .map_err(BotError::IoError)?;
            if length == 0 {
                return Ok(());
            }

            if let Some(b'\n') = buf.last() {
                buf.pop();
            }
            if let Some(b'\r') = buf.last() {
                buf.pop();
            }

            let line = irctokens::Line::tokenise(&buf).map_err(BotError::IrcError)?;
            eprintln!("{:?}", line);
            match line.command.as_str() {
                "PING" => self.handle_ping(line).await?,
                "001" => self.handle_001(line).await?,
                "433" => self.handle_433(line).await?,
                "381" => self.handle_381(line).await?,
                "364" => self.handle_364(line).await?,
                "365" => self.handle_365(line).await?,
                "211" => self.handle_211(line).await?,
                "219" => self.handle_219(line).await?,
                _ => (),
            };

            buf.clear()
        }
    }
    async fn handle_ping(&self, mut line: Line) -> Result<(), BotError> {
        line.source = None;
        line.command.replace_range(1..2, "O");
        self.write_line(&line).await
    }
    /// welcome
    async fn handle_001(&self, _line: Line) -> Result<(), BotError> {
        eprintln!("connected!");
        self.begin().await?;
        Ok(())
    }
    /// nickname in use
    async fn handle_433(&self, line: Line) -> Result<(), BotError> {
        let mut nick = line.arguments[1].clone();
        nick.push(b'_');
        self.write_line(&Line {
            tags: None,
            source: None,
            command: "NICK".to_string(),
            arguments: vec![nick],
        })
        .await
    }
    /// youreoper
    async fn handle_381(&self, _line: Line) -> Result<(), BotError> {
        Ok(())
    }
    /// links reply
    async fn handle_364(&self, line: Line) -> Result<(), BotError> {
        let [_, left, right, ..] = line.arguments.as_slice() else {
            panic!("missing links parameters")
        };
        utf8ify!(left, right);

        let (lid, rid) = {
            let mut links = self.links.lock().await;
            links.add_link(left, right)
        };
        if lid != rid {
            // store our own copy of the links, as a directed (from the perspective of the current
            // server) graph, since diameter::SpanningTree only stores an undirected graph and that
            // makes it more annoying to output to graphviz
            let mut rlinks = self.rlinks.lock().await;
            if let Some(rlink) = rlinks.get_mut(&rid) {
                rlink.push(lid);
            } else {
                rlinks.insert(rid, vec![lid]);
            }
        }

        Ok(())
    }
    /// end of links
    async fn handle_365(&self, _line: Line) -> Result<(), BotError> {
        let links = self.links.lock().await;
        let (tree, names) = links.tree();
        eprintln!("{:?} {:?}", tree, names);
        for right in self.rlinks.lock().await.keys() {
            self.write_line(&Line {
                tags: None,
                source: None,
                command: "STATS".to_string(),
                arguments: vec![b"l".to_vec(), names[*right].as_bytes().to_vec()],
            })
            .await?
        }
        Ok(())
    }
    /// stats link info
    async fn handle_211(&self, line: Line) -> Result<(), BotError> {
        let [_, left, _, _, _, _, _, uptime] = line.arguments.as_slice() else {
            panic!("wrong number of args");
        };
        let right = line.source.unwrap();
        utf8ify!(left, right, uptime);
        let uptime = uptime.split(' ').next().unwrap();
        let uptime: usize = uptime.parse().unwrap();
        eprintln!("{} {} {}", left, right, uptime);
        let (Some(lid), Some(rid)) = ({
            let links = self.links.lock().await;
            (links.get_id(left), links.get_id(right))
        }) else {
            return Ok(());
        };
        self.uptimes.lock().await.insert((rid, lid), uptime);
        Ok(())
    }
    /// end of stats
    async fn handle_219(&self, line: Line) -> Result<(), BotError> {
        let Some(stype) = line.arguments.get(1) else {
            return Ok(());
        };
        if stype != b"l" {
            return Ok(());
        }
        {
            let mut rec = self.received.lock().await;
            *rec += 1;
            if *rec < self.rlinks.lock().await.len() {
                return Ok(());
            }
        }
        self.write_line(&Line {
            tags: None,
            source: None,
            command: "QUIT".to_string(),
            arguments: vec![b"meow meow meow meow".to_vec()],
        })
        .await
    }
}

#[derive(Debug)]
pub enum BotError {
    IoError(io::Error),
    IrcError(irctokens::tokenise::Error),
    Utf8Error(std::str::Utf8Error),
}

fn time_now() -> DateTime<Utc> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("ur a time traveller");
    DateTime::from_timestamp(now.as_secs() as i64, 0).unwrap()
}

fn display(secs: usize) -> (String, f32) {
    let hue = 0.3 - 0.3 * 0.999998_f32.powf(secs as f32);
    let (o, unit) = duration_simplify(secs);
    (format!("{o} {unit}{}", if o == 1 { "" } else { "s" }), hue)
}

fn duration_simplify(secs: usize) -> (usize, &'static str) {
    macro_rules! durations {
        ($in:expr, $(($unit:expr, $time:expr)),*) => {
            $(
                if $in >= $time {
                    return ($in/$time, $unit);
                }
            )*
        };
    }
    durations!(secs, ("week", 604800), ("day", 86400), ("hour", 3600));
    (secs / 60, "minute")
}

#[tokio::main]
async fn main() {
    let args = Opt::parse();
    let bot = Bot::connect(&args.addr, "linkuptime").await.unwrap();
    bot.run().await.unwrap();
    println!("{}", bot.finish().await)
}
