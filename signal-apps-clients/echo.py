import asyncio
import json
import os
import struct
import signal
import socket
import sys

from pathlib import Path
from typing import TypedDict

import argparse

class Response(TypedDict):
    type: str # "response", "control"
    value: str


async def client_connected_cb(reader, writer):
    while True:
        print("!");
        length = await reader.readexactly(4)
        if not length:
            break

        length = struct.unpack("!i", length)[0]
        if length > 1024:
            print("invalid length:", length)
            break
        print("got length:", length)

        line = await reader.readexactly(length)
        print("got line:", line)
        if not line:
            break

        try:
            line = json.loads(line)
        except json.decoder.JSONDecodeError:
            break

        if line["type"] == "query":
            response = "A simple echo app"
        elif line["type"] == "start":
            response = "Started echo!"
        else:
            response = line["data"]

        response = json.dumps(
            Response({"type": "response", "value": response})
        ).encode()
        if len(response) > 1024:
            response = json.dumps(
                Response({
                    "type": "response",
                    "value": "Sorry that's too long!"
                })
            ).encode()

        length = struct.pack("!i", len(response))
        writer.write(length)
        writer.write(response)

        await writer.drain()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('config', type=str, help='path to config json')
    args = parser.parse_args()

    with open(args.config) as f:
        config = json.load(f)

    appdir = Path(config["appdir"])
    socketpath = appdir / Path("echo")
    print(socketpath)
    # Make sure the socket does not already exist
    try:
        os.unlink(socketpath)
    except OSError:
        if os.path.exists(socketpath):
            raise

    loop = asyncio.get_event_loop()
    loop.create_task(
        asyncio.start_unix_server(client_connected_cb, path=socketpath)
    )

    def cleanup():
        os.unlink(socketpath)
        sys.exit(0)
    loop.add_signal_handler(signal.SIGINT, cleanup)
    loop.run_forever()

if __name__ == "__main__":
    main()
