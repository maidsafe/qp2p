# Changelog

All notable changes to this project will be documented in this file. See [standard-version](https://github.com/conventional-changelog/standard-version) for commit guidelines.

### [0.9.23](https://github.com/maidsafe/qp2p/compare/v0.9.22...v0.9.23) (2021-02-21)

### [0.9.22](https://github.com/maidsafe/qp2p/compare/v0.9.21...v0.9.22) (2021-02-20)


### Features

* **igd:** expose a feature to completely disable IGD support, downgrading igd crate to v0.11.1 ([0873b29](https://github.com/maidsafe/qp2p/commit/0873b29972f8c179bee585ed45f913cf0280bf61))


### Bug Fixes

* **echo_service:** dont contact echo_service if IGD was successful ([8899436](https://github.com/maidsafe/qp2p/commit/88994366c8a995fc941b9e8936fd3530ea18c24d))

### [0.9.21](https://github.com/maidsafe/qp2p/compare/v0.9.20...v0.9.21) (2021-02-19)


### Bug Fixes

* **config:** do not use the IGD gateway to realize local IP address ([62dbcc4](https://github.com/maidsafe/qp2p/commit/62dbcc4d9eda960adabb76c13c10e98ab4842735))

### [0.9.20](https://github.com/maidsafe/qp2p/compare/v0.9.19...v0.9.20) (2021-02-19)


### Bug Fixes

* use loopback even when IGD finds local ip, if network is loopback ([95f00b3](https://github.com/maidsafe/qp2p/commit/95f00b33739043d03bc8f382340c68e9f2b126d0))

### [0.9.19](https://github.com/maidsafe/qp2p/compare/v0.9.18...v0.9.19) (2021-02-18)


### Bug Fixes

* **logs:** dont log expected messages at error level ([12b2c5a](https://github.com/maidsafe/qp2p/commit/12b2c5a6cca8b6b85b6239bc37e53a79df768921))

### [0.9.18](https://github.com/maidsafe/qp2p/compare/v0.9.17...v0.9.18) (2021-02-16)

### [0.9.17](https://github.com/maidsafe/qp2p/compare/v0.9.16...v0.9.17) (2021-02-15)


### Bug Fixes

* use hard coded contacts to realize local IP address ([3741833](https://github.com/maidsafe/qp2p/commit/3741833f787a44781509ed4ca8006f09aa7c5814))

### [0.9.16](https://github.com/maidsafe/qp2p/compare/v0.9.15...v0.9.16) (2021-02-12)


### Features

* adds a p2p node example that uses bidirectional streams ([46184b8](https://github.com/maidsafe/qp2p/commit/46184b85fa99f9182f8a18c12b3a65c510f38485))
* makes QuicP2p::Endpoint cloneable so that it can more easily be used across threads. ([a8c8b9d](https://github.com/maidsafe/qp2p/commit/a8c8b9dbeb965b609d280205a0bb6258cb40a6c8))
* **api:** add API to open bi-directional stream that can be used to ([158ae88](https://github.com/maidsafe/qp2p/commit/158ae88874aef3d7ba9f8064fc95d46ec57ac866))
* **api:** add support for manual port forwarding by passing additional ([9dca7b9](https://github.com/maidsafe/qp2p/commit/9dca7b933ceb27b7fe7790f58b92100d09a248a8))
* **api:** move all connection handling and message handling internally ([0093c20](https://github.com/maidsafe/qp2p/commit/0093c20a468a85dba9529b1c0b9b4ef4a4cf4d23))
* **disconnects:** add API for disconnection and fix tests ([37abcf9](https://github.com/maidsafe/qp2p/commit/37abcf91bd42c9e35bbc7381f0ef310be7d8d960))
* **echo_service:** find if peer is externally reachable if external IP ([a9989cc](https://github.com/maidsafe/qp2p/commit/a9989cc43c12a8c580666da4b2447f81c65e7922))
* **echo_service:** perform UPnP and/or echo_service when the endpoint ([5812f7b](https://github.com/maidsafe/qp2p/commit/5812f7b46fbb76f7126d16213e05a2e0531194ef))


### Bug Fixes

* **all:** remove FIFO queues and use the mpsc channels directly ([2bab054](https://github.com/maidsafe/qp2p/commit/2bab0548d5131a36bf346589422c667fc87196f0))
* **echo_service:** prevent contacting the echo service multiple times ([f9cf906](https://github.com/maidsafe/qp2p/commit/f9cf906270faad885f67ce4e4b71bb3767e35f19))
* **example:** dont use LocalHost in example ([2d70c05](https://github.com/maidsafe/qp2p/commit/2d70c0591cf0659b90baa153d82ae71d0d9cf7f3))
* **igd:** don't skip port forwarding if local IP address is specified ([2fb6401](https://github.com/maidsafe/qp2p/commit/2fb6401eddbcb711c07f9b3a8deb4e9e0480bb1d))
* **test:** refactor structure of test code and fix echo_service test ([591ebf8](https://github.com/maidsafe/qp2p/commit/591ebf83529419ebe19701455a875533055c81b5))

### [0.9.15](https://github.com/maidsafe/qp2p/compare/v0.9.14...v0.9.15) (2021-02-09)

### [0.9.14](https://github.com/maidsafe/qp2p/compare/v0.9.13...v0.9.14) (2021-02-02)

### [0.9.13](https://github.com/maidsafe/qp2p/compare/v0.9.12...v0.9.13) (2021-01-27)


### Bug Fixes

* **connections:** when gracefully finishing uni-stream upon sending bytes, do not remove conn from the pool if an error thrown was caused due to being already closed ([e6c5a2a](https://github.com/maidsafe/qp2p/commit/e6c5a2a08e41191e59684d7b055d37e624ef624c))

### [0.9.12](https://github.com/maidsafe/qp2p/compare/v0.9.11...v0.9.12) (2021-01-21)

### [0.9.11](https://github.com/maidsafe/qp2p/compare/v0.9.10...v0.9.11) (2021-01-20)

### [0.9.10](https://github.com/maidsafe/qp2p/compare/v0.9.9...v0.9.10) (2021-01-20)


### Features

* get peer's cached connection ([1d8f4ab](https://github.com/maidsafe/qp2p/commit/1d8f4abda9bb8b7485a94e0bed7e5f9c4ea76c1e))

### [0.9.9](https://github.com/maidsafe/qp2p/compare/v0.9.8...v0.9.9) (2021-01-14)

### [0.9.8](https://github.com/maidsafe/qp2p/compare/v0.9.7...v0.9.8) (2021-01-13)

### [0.9.7](https://github.com/maidsafe/qp2p/compare/v0.9.6...v0.9.7) (2020-12-29)

### [0.9.6](https://github.com/maidsafe/qp2p/compare/v0.9.5...v0.9.6) (2020-12-10)

### [0.9.5](https://github.com/maidsafe/qp2p/compare/v0.9.4...v0.9.5) (2020-12-10)

### [0.9.4](https://github.com/maidsafe/qp2p/compare/v0.9.3...v0.9.4) (2020-12-10)


### Features

* **api:** add more error variants and use them instead of ([bb56857](https://github.com/maidsafe/qp2p/commit/bb56857054d214e2e5cac34f568c5b218c353fd5))


### Bug Fixes

* **example:** remove panics from the example ([20dfe02](https://github.com/maidsafe/qp2p/commit/20dfe02a603f5a2f9e33f5171fddebbbf3daabb9))

### [0.9.3](https://github.com/maidsafe/qp2p/compare/v0.9.2...v0.9.3) (2020-12-03)


### Bug Fixes

* proper error on empty bootstrap ([db61592](https://github.com/maidsafe/qp2p/commit/db6159250d3c035a1fab5ac4d6d55cc155cb6e9e))

### [0.9.2](https://github.com/maidsafe/qp2p/compare/v0.9.1...v0.9.2) (2020-11-24)

### [0.9.1](https://github.com/maidsafe/qp2p/compare/v0.9.0...v0.9.1) (2020-11-19)


### Bug Fixes

* do not initialize logger in QuicP2p constructor ([4952639](https://github.com/maidsafe/qp2p/commit/495263925d3f6bb4e3c544df0fa569d5ef085665))

## [0.9.0](https://github.com/maidsafe/qp2p/compare/v0.8.9...v0.9.0) (2020-11-19)


### ⚠ BREAKING CHANGES

* `QuicP2p::bootstrap` return type changed from `Result<(Endpoint, Connection)>` to `Result<(Endpoint, Connection, IncomingMessages)>`.
* Dropping `Connection` while some send/recv streams are still in scope no longer closes the connection. All those streams must be dropped too before the connection is closed.
*     - `Endpoint::connect_to` now returns pair or `(Connection, Option<IncomingMessages>)` (previously it returned only `Connection`).
    - `Connection::open_bi_stream` renamed to `open_bi` (same as in quinn)
    - `Connection::send` renamed to `send_bi` for consistency
    - `Endpoint::listen` no longer returns `Result`

### Features

* add Endpoint::close ([2cedb77](https://github.com/maidsafe/qp2p/commit/2cedb77f417c8e66fadf162b8e73aa04c71f5845))
* do not close `quinn::Connection` on `Connection` drop ([1e5ee89](https://github.com/maidsafe/qp2p/commit/1e5ee8988d29c90fb2080ad63342b35d3a5c6e3c))
* implement Clone for Connection ([51de4c8](https://github.com/maidsafe/qp2p/commit/51de4c865a5d0bdaf22d28c1886f011c39500476))
* implement connection deduplication ([175c563](https://github.com/maidsafe/qp2p/commit/175c5635db935b60bdf590ce99b23e196f6c0d0b))
* implement connection pooling ([6edb290](https://github.com/maidsafe/qp2p/commit/6edb29049a7ef13cd39ba0289a6983c165d7bafa))
* re-export quinn::ConnectionError ([c10895d](https://github.com/maidsafe/qp2p/commit/c10895da0042110b7d231483c6dcd2bf00adcb0e))
* remove connection from the pool when manually closed ([815eb11](https://github.com/maidsafe/qp2p/commit/815eb110f59e1511f61b251975016fd25073df8b))
* return also IncomingMessages from QuicP2p::bootstrap ([cd18837](https://github.com/maidsafe/qp2p/commit/cd18837e81033b5753d926bd475a2289ab29c997))

### [0.8.9](https://github.com/maidsafe/qp2p/compare/v0.8.8...v0.8.9) (2020-11-18)

### [0.8.8](https://github.com/maidsafe/qp2p/compare/v0.8.7...v0.8.8) (2020-11-10)


### Features

* **config:** add --fresh and --clean flags to the config to prevent use ([b685d77](https://github.com/maidsafe/qp2p/commit/b685d77a93099d44d56624704545a6e043758711))
* **port_forwarding:** refactor IGD and echo service to be async ([a19cd51](https://github.com/maidsafe/qp2p/commit/a19cd51e160f531571f6ba91a6c0ce7a672e66b7))
* **upnp:** add config to disable port forwarding for clients ([4a14488](https://github.com/maidsafe/qp2p/commit/4a144886cc1924ea51fa783625551849be8cab84))


### Bug Fixes

* **echo_service:** respond to echo service request and expand test ([40217e1](https://github.com/maidsafe/qp2p/commit/40217e1fa4f666ddd3b4decff1b94a9b29e685e4))
* **echo_service_test:** refactor test to use tokio::spawn and join! ([2f82af2](https://github.com/maidsafe/qp2p/commit/2f82af2047b186d996c5e139e57160cf1cdda4e0))
* **examples:** Fix clippy errors in examples ([c201df3](https://github.com/maidsafe/qp2p/commit/c201df3bfb6d1f8ec50274c8637937d85036895c))
* **upnp:** add timeout for IGD and echo service ([2167ead](https://github.com/maidsafe/qp2p/commit/2167eade5d2f72c4d7efdeb81c14f88cfd89ff39))

### [0.8.7](https://github.com/maidsafe/qp2p/compare/v0.8.6...v0.8.7) (2020-11-05)

### [0.8.6](https://github.com/maidsafe/qp2p/compare/v0.8.5...v0.8.6) (2020-10-27)

### [0.8.5](https://github.com/maidsafe/qp2p/compare/v0.8.4...v0.8.5) (2020-10-26)

### [0.8.4](https://github.com/maidsafe/qp2p/compare/v0.8.3...v0.8.4) (2020-09-30)

### [0.8.3](https://github.com/maidsafe/qp2p/compare/v0.8.2...v0.8.3) (2020-09-24)

### [0.8.2](https://github.com/maidsafe/qp2p/compare/v0.8.1...v0.8.2) (2020-09-22)


### Features

* **api:** change bootstrap_nodes arg in with_config API to be an slice rather than a VecDeque ([a505065](https://github.com/maidsafe/qp2p/commit/a50506513a0f2623d0ae00a359b5d4ac167ccfb0))
* **header:** add message header that is sent over the wire for data ([4dc09b8](https://github.com/maidsafe/qp2p/commit/4dc09b8b53c3557bb270d84b45e77872cc084394))


### Bug Fixes

* **client-ip:** set to use loopback ip if hard coded contacts are loopback ([06fb27f](https://github.com/maidsafe/qp2p/commit/06fb27f7a24d4030029d1739746f18344c7b65b2))
* **endpoint:** return error if no local addr was specified and IGD is not available ([940dce9](https://github.com/maidsafe/qp2p/commit/940dce912d96bbf61db3eae622587f4984c0d041))
* **log:** minor fixes in log messages ([0a9bf09](https://github.com/maidsafe/qp2p/commit/0a9bf09f7a628843e014302e81d21070af5a5566))

### [0.8.1](https://github.com/maidsafe/qp2p/compare/v0.8.0...v0.8.1) (2020-09-08)
* Update repo/crate name to qp2p

### [0.8.0](https://github.com/maidsafe/qp2p/compare/0.7.0...v0.8.0) (2020-09-08)
* Update repo/crate name to quic_p2p to match org naming convention
* Refactor the API to allow reusing of streams to exchange multiple messages
* Refactor and fix tests to use the new API
* Add api to get connection stream without sending a message
* Update qp2p endpoint port when a random port is used
* Expose a function to query remote address from a Connection
* Add support for listening to messages from both uni-streams and bi-streams
* Expose a 'listen' API which return a stream of connections and in turn messages
* Support for bootstrapping using multiple nodes concurrently
* Support sending a message on a Connection and awaiting for a response using unidirectional streams
* Expose an async API

### [0.7.0]
* Standardize cargo dependency versioning
* Return an error when IGD fails

### [0.6.2]
* Fix clippy errors in feature-gated code
* Fix bug in get_connection_info with `upnp` enabled.

### [0.6.1]
* Skip port forwarding if quic-p2p is running on the loopback address.

### [0.6.0]
* Include support for UPnP and improve echo service.
* Use IGD for port forwarding and use the IGD gateway to find a node's local IP address.

### [0.5.0]
* Update quinn to 0.6.0
* Update rustls to 0.17.0

### [0.4.0]
* Force the use of the basic single-threaded Tokio scheduler to prevent conflicts when used by a crate using Tokio `rt-threaded` feature
* Take two channels, one for client event and a second one for a node
* Use node or client channel for sending a message depending on the peer we are receiving the message from
* Remove the use of peer certificate, and therefore remove it from the handshake process
* Use shared QUIC `ClientConfig` instead of one per peer
* Update for Rust 1.41 (mem::replace -> mem::take)
* Use structopt to parse command line arguments
* Rename `proxies` to `bootstrap_nodes`
* Migrate to async/await syntax with new quinn v0.5
* Update CI to run all packages in the worspace
* Migrate CI/CD pipeline to GitHub Actions
* Use new new-style macro import
* Unsent user messages in the pending queues of an ongoing connection attempt will now be sent back to the user library if the connection attempt fails.
* Report connection failure for all cases where the connection was initiated by us. Previously some of the cases where not handled.
* Fire unsent user messages to the clients back to the user library. Previously unsent messages to clients were silently ignored.

### [0.3.0]
* Expose `Dirs` and `OverRide` structs publicly.
* Add `boostrap_cache_dir` field to the config to specify a custom path for the bootstrap cache.

### [0.2.1]
* Fix incorrect deserialisation logic in `WireMsg`
* Fix `fmt::Display` for `Event` and `WireMsg`

### [0.2.0]
* Fix bugs
* Modify API and internals with changes required by routing
* Return user messages given via `send` API for both successful and unsuccessful sends
* Tie a user given token to the event returning the above message to help identify the context

### [0.1.1]
* Initial release.
* Implement bootstrap cache.
* Implement the bootstrap logic.
* Add peer types (Client/Node).
* Implement optimised user message transfer (for larger messages).
* Add utils for testing delayed connections.
* Add configuration loading from files and command line (with `structopt`).
