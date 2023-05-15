# Exchange Observer

Exchange Observer is a utility to track prices on various crypto exchanges in a
incredibly fast multithreaded way.

## About the project

`exch_observer` is suitable to be used in various trading bots (because of low
latency) and data-tracking (because of high throughput) applications. It can be
used as a library as well as using local (or remote) server through RPC client.

The `exch_observer` design allows to subscribe to high amount of trading pairs
at once (`>1300` on Binance and **all pairs** on Huobi), getting an instant
responses on updates using (where exists) WebSocket APIs.

## Usage

Trading bots is where `exch_observer` stands out, it can be used both as a
separate server (to avoid initializing WS connections every time on startup) or
within the bot (to reduce latency to minimum).

To create server you would first compile the CLI:

`$ cargo run build`

After that you would need to prepare configurations in your `~/.exch_observer`.
Scripts from [debug repo](https://github.com/theonekeyg/exch_observer_dbg) might be used
for that.

Next, launch the observer:

`$ RUST_LOG=$LOG_VERBOSITY ./target/exch_cli launch`

To test it, either use the CLI:

`$ exch_cli fetch-symbol --base eth --quote usdt --network huobi`

Or run use CLI from `exch_observer's` debug repo.

## Contribution

`exch_observer` is still in active development, so ABIs and APIs is still a concept of
change. But contributing implementations of observers as well as proposals for
handling DEXs are greatly appreciated.
