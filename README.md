# Signal Apps

A framework for writing bot based apps in Signal.

The idea is that a user would set up an instance and install apps they are
interested in and would then invite others to use their instance. Maybe some
apps would even allow for interaction between users connected to the same
instance.

## Overview

Each app can be implemented as a "client" which communicates with the "server"
which manages the interaction with `signal-cli` to handle sending and receiving
messages from users.

Each "client" app must create a unix domain socket which the "server" can
connect to. See [protocol.md](./protocol.md) to see how they communicate.


`signal-apps-server/` contains the implementation for the server

`signal-apps-clients/` contains some sample apps:

+ `signal-apps-clients/echo`
  + A simple echo server
+ `signal-apps-clients/tictactoe`
  + A multiplayer tictactoe game

## Building/running

### First time setup

This process is a bit convoluted at the moment. It will be simplified in the
future.

+ build `signal-cli`
+ Obtain a mobile number to register the server with on Signal
+ Register `signal-cli` as the Signal client for that number and make sure you
  can send and receive messages from it.
+ Using `signal-cli` send a message to the account you intend to use to interact
  with the server
+ Build the server

### Running the server

+ Launch the server, giving it a config file telling it where to find app
  sockets and what user to register as (see `config.json`).
+ Launch any clients you want to try out, giving them the same config file.
