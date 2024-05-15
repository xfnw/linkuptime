#!/usr/bin/env python3

import asyncio, datetime, sys, re

from irctokens import build
from ircrobots import Bot as BaseBot
from ircrobots import Server as BaseServer
from ircrobots import ConnectionParams
from ircstates.server import ServerDisconnectedException


def eprint(*args, **rawr):
    print(*args, file=sys.stderr, **rawr)


class Server(BaseServer):
    def __init__(self, bot, name):
        super().__init__(bot, name)
        self.linkconns = {}
        self.linkstats = {}
        self.statlcount = 0

    async def line_read(self, line):
        eprint(f"{self.name} < {line.format()}")
        fun = "on_" + line.command.lower()
        if fun in dir(self):
            asyncio.create_task(getattr(self, fun)(line))

    async def line_send(self, line):
        eprint(f"{self.name} > {line.format()}")

    async def on_001(self, line):
        eprint(f"connected to {self.isupport.network}")
        self.starttime = datetime.datetime.now(datetime.timezone.utc)
        await self.send_raw("LINKS")

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
            loop = asyncio.get_running_loop()
            loop.stop()

    async def generate_output(self):
        print("graph u {")

        for right, peers in self.linkconns.items():
            for left in peers:
                if (right, left) in self.linkstats:
                    up = round(self.linkstats[(right, left)] / 3600)
                    print(f'"{right}" -- "{left}" [label="{up} hours"];')
                    continue
                print(f'"{right}" -- "{left}";')

        now = datetime.datetime.now(datetime.timezone.utc)
        delta = (now - self.starttime).seconds
        now = now.strftime("%Y-%m-%d %H:%M:%SZ")
        print(f'"generated {now}\\n{delta} seconds elapsed" [shape="box"];')
        print("}")


class Bot(BaseBot):
    def create_server(self, name: str):
        return Server(self, name)


async def connect(host):
    bot = Bot()
    params = ConnectionParams("linkuptime", host, 6697)
    await bot.add_server("yip", params)

    await bot.run()


def main():
    try:
        asyncio.run(connect(sys.argv[1]))
    except RuntimeError:
        pass


if __name__ == "__main__":
    main()
