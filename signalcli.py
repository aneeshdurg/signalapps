import json

import os
import subprocess
import threading

from queue import Queue
from typing import Callable

from sender import Sender
from receiver import Receiver

SIGNALCLI_PATH = "build/install/signal-cli/bin/signal-cli"
def do_chdir():
    os.chdir("signal-cli")


class SignalCliDaemon:
    def __init__(self, user: str) -> None:
        self.lock = threading.Lock()

        self.user = user

        self.recv_cmd = [
            SIGNALCLI_PATH,
            "--output=json",
            "-u",
            self.user,
            "receive",
            "--timeout",
            "-1", # disables timeout
        ]
        self._stop_recv = False

        self.send_cmd = [
            SIGNALCLI_PATH,
            "-u",
            self.user,
            "send",
            "-m"
        ]

        self.on_msg: Callable[[bytes], None] = lambda _: None

        # the reciever runs persistantly in the background until a send request
        # is sent. Then the receiver is stopped the message is sent and the
        # reciever is restarted.
        self.start_recv()

    def set_on_msg(self, on_msg_cb: Callable[[bytes], None]) -> None:
        self.on_msg = on_msg_cb

    def start_recv(self):
        with self.lock:
            self.cli_proc = subprocess.Popen(
                self.recv_cmd, stdout=subprocess.PIPE, preexec_fn=do_chdir
            )

            def reader_thread():
                while not self._stop_recv:
                    msg = self.cli_proc.stdout.readline()
                    if msg:
                        print("daemon got msg", msg)
                        self.on_msg(msg)
            self.reader_thread = threading.Thread(target=reader_thread)
            self.reader_thread.start()

    def _do_stop_recv(self):
        self._stop_recv = True

        print("Waiting for cliproc");
        self.cli_proc.terminate()
        self.cli_proc.wait()
        print("done Waiting for cliproc");

        print("Waiting for rthread");
        self.reader_thread.join()
        print("done Waiting for rthread");
        self._stop_recv = False

    def stop_recv(self):
        with self.lock:
            self._do_stop_recv()

    def send(self, dest: str, msg: str) -> None:
        with self.lock:
            self._do_stop_recv()
            subprocess.check_call(
                self.send_cmd + [msg, dest], preexec_fn=do_chdir)
        self.start_recv()


class SignalCliReceiver(Receiver):
    def __init__(self, daemon: SignalCliDaemon):
        self.daemon = daemon

        self.msgs = Queue()
        self.cbs = []

        self.daemon.set_on_msg(lambda msg: self.msgs.put(msg))

    def start(self) -> None:
        def cb_thread():
            while True:
                msg = self.msgs.get()
                print("recv got msg")
                if msg is None:
                    break
                msg = json.loads(msg.decode())
                for cb in self.cbs:
                    cb(msg)
        self.cb_thread = threading.Thread(target=cb_thread)
        self.cb_thread.start()

    def stop(self) -> None:
        self.daemon.stop_recv()
        self.msgs.put(None)
        self.cb_thread.join()

    def add_cb(self, on_msg_cb: Callable[[dict], None]) -> None:
        self.cbs.append(on_msg_cb)


class SignalCliSender(Sender):
    def __init__(self, daemon) -> None:
        self.daemon = daemon

    def send(self, dest: str, msg: str) -> None:
        self.daemon.send(dest, msg)
