import random
import re
import textwrap

from enum import Enum
from queue import Queue
from threading import Thread
from typing import Callable, Dict, List, Optional, Set, Type

from app import App, AppServer
from sender import NullSender, Sender


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


class TicTacToe(App):
    def __init__(
        self,
        server: AppServer,
        source: str,
        content: str,
        sender: Sender,
        terminate_cb: Callable[[], None]
    ) -> None:
        super().__init__(server, source, content, sender, terminate_cb)

        assert isinstance(self.server, TicTacToeServer)
        self.lobby = self.server.lobby

        self.state = State.MAINMENU
        self.opponent = None

        self.stopped = False

        self.board = None
        self.boardid = None
        self.idx = None

    def start(self) -> None:
        self.sender.send(self.user, "Welcome to tic tac toe!")
        self.send_mainmenu()

    def send_mainmenu(self) -> None:
        self.sender.send(
            self.user,
            "Send (a) to join the lobby. Send (b) to play against a computer."
        )

    def send_lobby_msg(self):
        self.sender.send(
            self.user, "You are currently in the lobby. Send 'q' to exit"
        )

    def send_help_msg(self):
        helpmsg = textwrap.dedent(
        """\
            Commands:
                m [x] [y] - mark cell (x, y)
                    (where top left corner is (0,0), bottom right is (2, 2))
                f - forfeit and quit
        """
        )
        self.sender.send(self.user, helpmsg)

    def recv(self, msg: str) -> None:
        if self.state == State.MAINMENU:
            if msg == 'a':
                self.state = State.LOBBY
                self.lobby.add(self)
                self.send_lobby_msg()
            elif msg == 'b':
                self.state = State.LOBBY
                board = Board('COM', [self, TicTacToeCom(self.server)])
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
        self.sender.send(self.user, msg)

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
    def __init__(self, server) -> None:
        super().__init__(
            server, "COM", "", NullSender(), lambda : None
        )
        self.board = None

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


class TicTacToeServer(AppServer):
    name = "TicTacToe"
    desc = "tic tac toe! Play alone or with others"

    def __init__(self, lobby: Lobby) -> None:
        self.lobby = lobby
        self.lobby.start()

    def vend(self) -> Type[App]:
        return TicTacToe

    def stop(self) -> None:
        self.lobby.stop()
