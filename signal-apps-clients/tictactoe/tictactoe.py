import argparse
import json
import os
import random
import re
import signal
import socket
import struct
import textwrap

from abc import abstractmethod
from pathlib import Path

from enum import Enum
from queue import Queue
from threading import Thread
from typing import Callable, Dict, List, Optional, Set, Type


class State(Enum):
    MAINMENU = "MAINMENU"
    LOBBY = "LOBBY"
    GAME = "GAME"


class Board:
    letters = ['X', 'O']

    def __init__(self, gameid: object, players: 'TicTacToe') -> None:
        self.id = gameid

        self.state = [
            [None, None, None],
            [None, None, None],
            [None, None, None]
        ];

        for idx, player in enumerate(players):
            player.setboard(self, idx)
            player.board_send(self.id, "Connected! Starting game.")
        self.players = players
        self.turn = 0
        self.send_board()

    def send_board(self) -> None:
        board = ""
        for i in range(3):
            board += "|"
            for j in range(3):
                cell = self.state[i][j]
                board += ' ' if cell is None else self.letters[cell]
                board += '|'
            board += '\n'

        winner = self.end_condition()
        statuses = ['', '']
        end = False
        if winner is not None:
            end = True
            loser = (winner + 1) % 2

            statuses[winner] = "Congratulations! You won!"
            statuses[loser] = "You lost, better luck next time!"
        else:
            for idx, player in enumerate(self.players):
                statuses[idx] = "Please wait for the other player"
                if self.turn == idx:
                    statuses[idx] = "It's your turn!"

        for idx, player in enumerate(self.players):
            player.board_send(self.id, board + "\n" + statuses[idx])
            if end:
                player.gameover()

    def move(self, idx: int, movex: int, movey: int) -> None:
        if self.turn != idx:
            self.players[idx].board_send(self.id, "Please wait for your turn")
            return

        if movex not in range(3) or movey not in range(3):
            self.players[idx].board_send(self.id, "Invalid move")
            return

        if self.state[movey][movex] is not None:
            self.players[idx].board_send(self.id, "Invalid move")
            return

        self.state[movey][movex] = idx

        self.turn += 1
        self.turn %= 2
        self.send_board()

    def end_condition(self) -> Optional[int]:
        def check_end(line: List[int]) -> None:
            if line[0] is not None and line[0] == line[1] == line[2]:
                return line[0]
            return None
        # check horzt
        for i in range(3):
            row = self.state[i]
            col = [self.state[j][i] for j in range(3)]

            for line in [row, col]:
                res = check_end(line)
                if res is not None:
                    return res

        diag_1 = [self.state[0][0], self.state[1][1], self.state[2][2]]
        diag_2 = [self.state[2][0], self.state[1][1], self.state[0][2]]
        for line in [diag_1, diag_2]:
            res = check_end(line)
            if res is not None:
                return res
        return None


    def forfeit(self, idx: int) -> None:
        other = (idx + 1) % 2
        self.players[other].board_send(
            self.id, "The other player has forfeited! You win!\n")
        self.players[other].gameover()

class Lobby:
    def __init__(self) -> None:
        self.players = Queue()
        self.active_players = set()

        self.thread = Thread(target=self.poll_lobby)
        self.started = True

        self.gameid = 0

    def start(self) -> None:
        self.thread.start()

    def stop(self) -> None:
        self.players.put(None)
        self.thread.join()

    def add(self, player: 'TicTacToe') -> None:
        self.players.put(player)
        self.active_players.add(player)

    def remove(self, player: 'TicTacToe') -> None:
        try:
            self.active_players.remove(player)
        except KeyError:
            # There's a race!
            # TODO there needs to be something in place to prevent the game from
            # starting if this happens
            pass

    def poll_lobby(self) -> None:
        done = False
        while True:
            players = []
            while len(players) != 2:
                player = self.players.get()
                if player is None:
                    done = True
                    break

                # The other player might have disconnected
                if len(players) and players[0] not in self.active_players:
                    players = []

                # The current player might have disconnected
                if player not in self.active_players:
                    continue

                players.append(player)

            if done:
                break

            for player in players:
                self.active_players.remove(player)

            # Start a game with players
            Board(self.gameid, players)
            self.gameid += 1


class Sender:
    @abstractmethod
    def send(self, msg: str) -> None:
        ...

    @abstractmethod
    def terminate(self, reason: Optional[str]) -> None:
        ...

class NullSender(Sender):
    def send(self, msg: str) -> None:
        pass

    def terminate(self, reason: Optional[str]) -> None:
        pass


class TicTacToe:
    def __init__(
        self, lobby: Optional[Lobby], source: str, sender: Sender
    ) -> None:
        self.sender = sender
        self.user = source
        self.lobby = lobby

        self.state = State.MAINMENU
        self.opponent = None

        self.stopped = False

        self.board = None
        self.boardid = None
        self.idx = None

    def start(self) -> None:
        self.sender.send("Welcome to tic tac toe!")
        self.send_mainmenu()

    def send_mainmenu(self) -> None:
        self.sender.send(
            "Send (a) to join the lobby. Send (b) to play against a computer."
        )

    def send_lobby_msg(self):
        self.sender.send("You are currently in the lobby. Send 'q' to exit")

    def send_help_msg(self):
        helpmsg = textwrap.dedent(
        """\
            Commands:
                m [x] [y] - mark cell (x, y)
                    (where top left corner is (0,0), bottom right is (2, 2))
                f - forfeit and quit
        """
        )
        self.sender.send(helpmsg)

    def recv(self, msg: str) -> None:
        if self.state == State.MAINMENU:
            if msg == 'a':
                self.state = State.LOBBY
                self.lobby.add(self)
                self.send_lobby_msg()
            elif msg == 'b':
                self.state = State.LOBBY
                board = Board('COM', [self, TicTacToeCom()])
            else:
                self.send_mainmenu()
        elif self.state == State.LOBBY:
            if msg != 'q':
                self.send_lobby_msg()
            else:
                self.lobby.remove(self)
                self.state = State.MAINMENU
                self.send_mainmenu()
        elif self.state == State.GAME:
            if msg == 'f':
                self.board.forfeit(self.idx)
                self.gameover()
            elif (match := re.match('m (\d+) (\d+)', msg)):
                groups = match.groups()
                self.board.move(self.idx, int(groups[0]), int(groups[1]))
            else:
                self.send_help_msg()

    def setboard(self, board: Board, idx: int) -> None:
        if self.stopped or self.state != State.LOBBY:
            board.forfeit(idx)
        else:
            self.board = board
            self.boardid = board.id
            self.idx = idx
            self.state = State.GAME

    def board_send(self, boardid: object, msg: str) -> None:
        if self.stopped or boardid != self.boardid:
            return
        self.sender.send(msg)

    def gameover(self) -> None:
        self.board = None
        self.boardid = None
        self.idx = None
        self.state = State.MAINMENU
        self.send_mainmenu()

    def stop(self) -> None:
        self.stopped = True
        if self.board:
            self.board.forfeit(self.idx)

class TicTacToeCom(TicTacToe):
    def __init__(self) -> None:
        super().__init__(None, "COM", NullSender())

    def setboard(self, board: Board, idx: int) -> None:
        self.board = board
        self.idx = idx

    def board_send(self, _id: object, msg: str) -> None:
        if "your turn" not in msg:
            return
        else:
            x = None
            y = None
            while True:
                x = random.randint(0, 2)
                y = random.randint(0, 2)
                if self.board.state[y][x] == None:
                    break
            self.board.move(self.idx, x, y)

    def gameover(self) -> None:
        pass


class TicTacToeServer:
    name = "tictactoe"
    desc = "tic tac toe! Play alone or with others"

    def __init__(self) -> None:
        self.lobby = Lobby()
        self.lobby.start()

    def stop(self) -> None:
        self.lobby.stop()


class SockSender(Sender):
    def __init__(self, connection) -> None:
        self.connection = connection

    def send(self, msg: str) -> None:
        response = json.dumps({"type": "response", "value": msg}).encode()
        length = struct.pack("!i", len(response))
        self.connection.sendall(length)
        self.connection.sendall(response)

    def terminate(self, reason: Optional[str]) -> None:
        # TODO
        pass

def handle_conn(connection, appserver):
    app = None
    sender = SockSender(connection)
    while True:
        length = connection.recv(4)
        length = struct.unpack("!i", length)[0]

        msg = connection.recv(length)
        print(msg)
        try:
            msg = json.loads(msg)
        except json.decoder.JSONDecodeError:
            break

        if msg["type"] == "query":
            sender.send(appserver.desc)
        elif msg["type"] == "start":
            app = TicTacToe(appserver.lobby, msg["user"], sender)
            app.start()
        else:
            if not app:
                connection.close()
                break;
            app.recv(msg["data"])

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument('config', type=str, help='path to config json')
    args = parser.parse_args()

    with open(args.config) as f:
        config = json.load(f)

    appdir = Path(config["appdir"])
    socketpath = appdir / Path('tictactoe')
    print(socketpath)
    # Make sure the socket does not already exist
    try:
        os.unlink(socketpath)
    except OSError:
        if os.path.exists(socketpath):
            raise

    appserver = TicTacToeServer()
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.bind(bytes(socketpath))
    sock.listen(100)

    def shutdown(*args):
        appserver.stop()
        sock.close()
        # TODO Call app.stop() for all running apps and close all conns
    signal.signal(signal.SIGINT, shutdown)


    while True:
        # TODO have a list of active apps so that we can cancel all the threads.
        try:
            connection, _ = sock.accept()
        except OSError:
            break
        thread = Thread(target=handle_conn, args=(connection, appserver))
        thread.daemon = True
        thread.start()
