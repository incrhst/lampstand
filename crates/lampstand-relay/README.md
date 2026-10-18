# lampstand-relay

Self-hosted sync relay for Lampstand (PRD CD-3). One 2 MB binary that keeps a
copy of each church's documents, so devices at home and at church sync
through it without being online at the same time. Runs on a Raspberry Pi or
the smallest VPS.

## Run it

```sh
# 1. Make a token for your church (keep it secret; every device needs it).
lampstand-relay gen-token grace > churches.txt

# 2. Start the relay.
lampstand-relay --churches-file churches.txt --data /var/lib/lampstand-relay
```

Or with Docker, from the repository root:

```sh
docker build -f crates/lampstand-relay/Dockerfile -t lampstand-relay .
docker run -d -p 7878:7878 -v lampstand-relay:/data \
  -e LAMPSTAND_RELAY_CHURCHES="grace=<token>" lampstand-relay
```

On the internet, put a TLS reverse proxy in front and give devices the
`wss://` address. With Caddy:

```
relay.example.org {
    reverse_proxy 127.0.0.1:7878
}
```

## Configuration

| Flag | Environment | Default |
| --- | --- | --- |
| `--listen` | `LAMPSTAND_RELAY_LISTEN` | `0.0.0.0:7878` |
| `--data` | `LAMPSTAND_RELAY_DATA` | `./lampstand-relay-data` |
| `--churches-file` | `LAMPSTAND_RELAY_CHURCHES_FILE` | none |
| | `LAMPSTAND_RELAY_CHURCHES` (`a=token,b=token`) | none |
| | `LAMPSTAND_RELAY_LOG` (`debug`, `info`, `warn`) | `info` |

One relay can serve several churches; each has its own token and data
directory and never sees another's documents. Tokens are not accepted on the
command line, where other users of the machine could read them.

## Security

The church token is the only access control: anyone holding it can read and
change all of that church's documents, and the relay operator can read them
too. Per-person invites and roles (CD-6) and end-to-end encryption for a
public relay (CD-4) are planned.
