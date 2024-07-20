#!/usr/bin/env python3

import argparse, asyncio, datetime, sys, re

from irctokens import build
from ircrobots import Bot as BaseBot
from ircrobots import Server as BaseServer
from ircrobots import ConnectionParams
from ircstates.server import ServerDisconnectedException


def eprint(*args, **rawr):
    print(*args, file=sys.stderr, **rawr)


def display(secs):
    hue = 0.3 - 0.3 * 0.999998**secs
    (o, unit) = duration_simplify(secs)
    return (f"{o} {unit}{'s' if o!=1 else ''}", hue)


def duration_simplify(t):
    if t >= 604800:
        t //= 604800
        return (t, "week")
    if t >= 86400:
        t //= 86400
        return (t, "day")
    if t >= 3600:
        t //= 3600
        return (t, "hour")
    t //= 60
    return (t, "minute")


class Server(BaseServer):
    def __init__(self, bot, name, darkmode=False, waitoper=False):
        super().__init__(bot, name)
        self.linkconns = {}
        self.linkstats = {}
        self.statlcount = 0
        self.darkmode = darkmode
        self.waitoper = waitoper

    async def line_read(self, line):
        eprint(f"{self.name} < {line.format()}")
        fun = "on_" + line.command.lower()
        if fun in dir(self):
            asyncio.create_task(getattr(self, fun)(line))

    async def line_send(self, line):
        eprint(f"{self.name} > {line.format()}")

    async def begin(self):
        self.starttime = datetime.datetime.now(datetime.timezone.utc)
        await self.send_raw("LINKS")

    async def on_001(self, line):
        eprint(f"connected to {self.isupport.network}")
        if not self.waitoper:
            await self.begin()

    async def on_381(self, line):
        if self.waitoper:
            await self.begin()

    async def on_364(self, line):
        [_, left, right, *_] = line.params

        if left == right:
            return

        if right in self.linkconns:
            self.linkconns[right].append(left)
        else:
            self.linkconns[right] = [left]

    async def on_365(self, line):
        eprint(self.linkconns)
        for name in self.linkconns:
            await self.send(build("STATS", ["l", name]))

    async def on_211(self, line):
        [_, left, _, _, _, _, _, uptime] = line.params
        right = line.hostmask.nickname

        self.linkstats[(right, left)] = int(uptime.split(" ")[0])

    async def on_219(self, line):
        if line.params[1] == "l":
            self.statlcount += 1
        if self.statlcount >= len(self.linkconns):
            await self.generate_output()
            await self.send_raw("QUIT :nuzzles u")

    async def generate_output(self):
        print('graph "' + self.name + '" {')
        if self.darkmode:
            print("bgcolor = black;")
            print('node [color=white;fontcolor=white;fontname="Comic Sans MS"];')
            print('edge [penwidth=2;color=gray;fontcolor=white;fontname="Comic Sans MS"];')
        else:
            print("edge [penwidth=2];")

        for right, peers in self.linkconns.items():
            for left in peers:
                if (right, left) in self.linkstats:
                    (up, hue) = display(self.linkstats[(right, left)])
                    print(f'"{right}" -- "{left}" [label="{up}";color="{hue},1,.8"];')
                    continue
                print(f'"{right}" -- "{left}";')

        now = datetime.datetime.now(datetime.timezone.utc)
        delta = (now - self.starttime).seconds
        now = now.strftime("%Y-%m-%d %H:%M:%SZ")
        print(f'"generated {now}\\n{delta} seconds elapsed" [shape="box"];')
        print("}")


class Bot(BaseBot):
    def __init__(self, darkmode=False, waitoper=False):
        super().__init__()
        self.darkmode = darkmode
        self.waitoper = waitoper

    def create_server(self, name: str):
        return Server(self, name, darkmode=self.darkmode, waitoper=self.waitoper)

    async def disconnected(self, server):
        if server.name in self.servers:
            del self.servers[server.name]
        if not self.servers:
            loop = asyncio.get_running_loop()
            loop.stop()


async def connect(host, darkmode=False, waitoper=False):
    bot = Bot(darkmode=darkmode, waitoper=waitoper)
    params = ConnectionParams("linkuptime", host, 6697)
    await bot.add_server("uppies", params)

    await bot.run()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("host")
    parser.add_argument("-d", help="enable dark mode", action="store_true")
    parser.add_argument("-o", help="wait for RPL_YOUREOPER", action="store_true")
    args = parser.parse_args()

    try:
        asyncio.run(connect(args.host, darkmode=args.d, waitoper=args.o))
    except RuntimeError:
        pass


if __name__ == "__main__":
    main()
