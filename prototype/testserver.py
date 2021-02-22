import json
import signal as ossignal

from app import EchoServer
from main import MainApp
from apps.tictactoe import TicTacToeServer, Lobby
from apps.admin import AdminServer
from signalcli import SignalCliDaemon, SignalCliReceiver, SignalCliSender

with open('user.json') as f:
  config = json.load(f)

daemon = SignalCliDaemon(config["username"])
recv = SignalCliReceiver(daemon)
send = SignalCliSender(daemon)
main = MainApp(config, recv, send, [
    AdminServer(config),
    EchoServer(),
    TicTacToeServer(Lobby())
])
def shutdown(*args):
    main.shutdown()
ossignal.signal(ossignal.SIGINT, shutdown)
