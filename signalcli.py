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
        self.user = user
        daemon_cmd = [SIGNALCLI_PATH, 'daemon']
        self.daemon = subprocess.Popen(
            daemon_cmd,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            preexec_fn=do_chdir
        )

    def stop(self):
        print("Waiting for daemon to stop");
        self.daemon.terminate()
        self.daemon.wait()
        print("done Waiting for daemon to stop");


class SignalCliReceiver(Receiver):
    def __init__(self, daemon: SignalCliDaemon):
        self.daemon = daemon

        self.msgs = Queue()
        self.cbs = []

        recv_cmd = [
            SIGNALCLI_PATH,
            "--dbus",
            "--output=json",
            "-u",
            self.daemon.user,
            "receive",
            "--timeout",
            "-1", # disables timeout
        ]

        self.recv_proc = subprocess.Popen(
            recv_cmd, stdout=subprocess.PIPE, preexec_fn=do_chdir
        )

        self._stop_recv = False
        def recv_thread():
            while not self._stop_recv:
                msg = self.recv_proc.stdout.readline()
                if msg:
                    self.msgs.put(msg)
        self.recv_thread = threading.Thread(target=recv_thread)
        self.recv_thread.start()

        def cb_thread():
            while True:
                msg = self.msgs.get()
                if msg is None:
                    break
                try:
                    msg = json.loads(msg.decode())
                    for cb in self.cbs:
                        cb(msg)
                except:
                    print("Could not decode msg")
                    print(msg)
        self.cb_thread = threading.Thread(target=cb_thread)
        self.cb_thread.start()

    def stop(self) -> None:
        self.daemon.stop()

        self._stop_recv = True
        self.recv_proc.terminate()
        self.recv_proc.wait()
        self.recv_thread.join()

        self.msgs.put(None)
        self.cb_thread.join()

    def add_cb(self, on_msg_cb: Callable[[dict], None]) -> None:
        self.cbs.append(on_msg_cb)


class SignalCliSender(Sender):
    def __init__(self, daemon) -> None:
        self.daemon = daemon

    def send(self, dest: str, msg: str) -> None:
        send_cmd = [
            SIGNALCLI_PATH,
            "--dbus",
            "-u",
            self.daemon.user,
            "send",
            "-m"
        ]
        print("Sending msg", dest, msg)

        subprocess.check_call(send_cmd + [msg, dest], preexec_fn=do_chdir)
        print("done")
