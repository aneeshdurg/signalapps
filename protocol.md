# Signal-apps Protocol

All messages are in the form 32bit big endian length followed by utf8 encoded 
json. The length is the length of the encoded json.

## Messages sent from the server to an app

### Query

This messages queries for an app's description.
This message should be able to be serviced without starting the app.

```
{
    "type": "query"
}
```

### Start

Notify an app that a connection should be associated with a running app.
```
{
    "type": "start"
    "user": username string
}
```

### msg

```
{
    "type": "msg"
    "data": Some data to be recieved by the app
}
```

### N.B.

There is no message for close. The connection will just be dropped instad.

## Messages sent from the app to the server

### response

This message should be forwarded directly to the user.

```
{
    "type": "response",
    "value": message for user
}
```

### terminate

(unimplemented)

Stop the app

```
{
    "type": "terminate",
    "reason": reason string
}
```
