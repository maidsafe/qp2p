# Changelog

All notable changes to this project will be documented in this file. See [standard-version](https://github.com/conventional-changelog/standard-version) for commit guidelines.

### [0.28.1](https://github.com/maidsafe/qp2p/compare/v0.28.0...v0.28.1) (2022-02-13)


### Features

* add IncomingConnections::try_recv() ([a7562e9](https://github.com/maidsafe/qp2p/commit/a7562e9468f6e5f7ec761818dc1ee8b40e1f1c38))

## [0.28.0](https://github.com/maidsafe/qp2p/compare/v0.27.4...v0.28.0) (2022-01-31)


### ⚠ BREAKING CHANGES

* moving to quinn 0.8

### Features

* moving to quinn 0.8 ([a9c0bb2](https://github.com/maidsafe/qp2p/commit/a9c0bb2fd70fcc659e1bb333f76aa345ec476ea7))


### Bug Fixes

* multiple conns to same peer test fixed ([5a6cb39](https://github.com/maidsafe/qp2p/commit/5a6cb39ef69efc1d0b53dcb8bf0fc05f67af2cc8))
* test and connection stability ([aee6dfc](https://github.com/maidsafe/qp2p/commit/aee6dfc23d47588528c6593e5539a554c3f1d8f0))

### [0.27.4](https://github.com/maidsafe/qp2p/compare/v0.27.3...v0.27.4) (2022-01-17)

### [0.27.3](https://github.com/maidsafe/qp2p/compare/v0.27.2...v0.27.3) (2022-01-11)


### Features

* add optional reason for closing a connection ([8ba42b5](https://github.com/maidsafe/qp2p/commit/8ba42b555bfca47e8835a9ee9707ef5b937d75b5))

### [0.27.2](https://github.com/maidsafe/qp2p/compare/v0.27.1...v0.27.2) (2021-11-04)


### Features

* implement `Debug` for `Connection` ([966567e](https://github.com/maidsafe/qp2p/commit/966567e4849ba6d8c8fca901a58de76724674059))

### [0.27.1](https://github.com/maidsafe/qp2p/compare/v0.27.0...v0.27.1) (2021-11-03)

## [0.27.0](https://github.com/maidsafe/qp2p/compare/v0.26.1...v0.27.0) (2021-10-28)


### ⚠ BREAKING CHANGES

* The `qp2p::config::DEFAULT_KEEP_ALIVE_INTERVAL`
constant has been removed, and the default value for
`keep_alive_interval` set for `Config::default()` is now `None`, meaning
keep-alives are disabled by default.

### Features

* add `Connection::id` to get a stable ID for a connection ([a86e558](https://github.com/maidsafe/qp2p/commit/a86e5587ae0f5f52f5941e51b759a860e997dba8))


### Bug Fixes

* more carefully handle 'benign' connection loss ([9e2be50](https://github.com/maidsafe/qp2p/commit/9e2be50c03e3e49bda7e12df7a2e13112ee89c90))


* disable keep-alives by default ([7788fda](https://github.com/maidsafe/qp2p/commit/7788fdafacaa0c96da7aa692d3ecb22fd1ad6c64))

### [0.26.1](https://github.com/maidsafe/qp2p/compare/v0.26.0...v0.26.1) (2021-10-28)

## [0.26.0](https://github.com/maidsafe/qp2p/compare/v0.25.0...v0.26.0) (2021-10-27)


### ⚠ BREAKING CHANGES

* Setting `qp2p::Config::keep_alive_interval = None` will
now disable keep-alives, rather than falling back to the default
interval.

### Features

* support disabling keep-alives ([dca54b3](https://github.com/maidsafe/qp2p/commit/dca54b3e281f3d788bccee26ce6d61733ca8cca3))

## [0.25.0](https://github.com/maidsafe/qp2p/compare/v0.24.0...v0.25.0) (2021-10-27)


### ⚠ BREAKING CHANGES

* - The `ConnId` trait has been removed.
- `Endpoint`, `IncomingConnections`, `Connection`, and
  `ConnectionIncoming` no longer have a generic type parameter.
- `Endpoint::disconnect_from`, `Endpoint::get_connection_by_addr`, and
  `Endpoint::get_connection_by_id` have been removed.
- `Connection::id` has been removed.
- `Endpoint::new`, `Endpoint::connect_to`, and
  `Endpoint::connect_to_any` now return
  `(Connection, ConnectionIncoming)`, rather than `(Connection,
  Option<ConnectionIncoming>)`.
- `Connection::open_bi` no longer takes a `priority` argument. This can
  be set with `SendStream::set_priority` instead.
- Semantically, all calls to `Endpoint::connect_to` and
  `Endpoint::connect_to_any` will establish and return new connections.
  There is no connection reuse.

### Bug Fixes

* drop an unused field from `tests::quinn::Peer` ([7b0fa53](https://github.com/maidsafe/qp2p/commit/7b0fa532d6aa3f1a048b05e0b2cb0498505efaf6))
* fix incorrect log message ([1a9261b](https://github.com/maidsafe/qp2p/commit/1a9261bf7b5280551086b4076a82e1023b7efe65))


* remove connection pooling ([fd19094](https://github.com/maidsafe/qp2p/commit/fd19094d5b5ea7ab13c71eda09eb01986ad738b8))

## [0.24.0](https://github.com/maidsafe/qp2p/compare/v0.23.0...v0.24.0) (2021-10-20)


### ⚠ BREAKING CHANGES

* `IncomingMessages` has been removed, and is no longer
returned by `Endpoint::new` or `Endpoint::new_client`.

### Bug Fixes

* don't retry sending on connection loss ([578936d](https://github.com/maidsafe/qp2p/commit/578936dc0bba0e53e6230e916038f90cde49cd33))


* remove `IncomingMessages` ([c0a6a20](https://github.com/maidsafe/qp2p/commit/c0a6a2042d0990b4ea7d550a1da7d5dcdf3bb559))

## [0.23.0](https://github.com/maidsafe/qp2p/compare/v0.22.2...v0.23.0) (2021-10-15)


### ⚠ BREAKING CHANGES

* `DisconnectionEvents` has been removed, and is no
longer returned from `Endpoint::new` or `Endpoint::new_client`.

### Bug Fixes

* fix an unread field lint on nightly ([6ebab02](https://github.com/maidsafe/qp2p/commit/6ebab0233bb494bffbc0cc3438aef246c25e7aa0))


* remove `DisconnectionEvents` ([93e70a7](https://github.com/maidsafe/qp2p/commit/93e70a7309fdef676b2be6b024e5faa15fe9e280))

### [0.22.2](https://github.com/maidsafe/qp2p/compare/v0.22.1...v0.22.2) (2021-10-13)


### Features

* add a counter for opened connections to `Endpoint` ([c6a9a42](https://github.com/maidsafe/qp2p/commit/c6a9a42b4efeb0af2ec1fb68367b996b09697eaa))

### [0.22.1](https://github.com/maidsafe/qp2p/compare/v0.22.0...v0.22.1) (2021-09-27)

## [0.22.0](https://github.com/maidsafe/qp2p/compare/v0.21.0...v0.22.0) (2021-09-27)


### ⚠ BREAKING CHANGES

* `IncomingConnections::next` now returns
`Option<Connection<I>>`. `IncomingMessages::next` now returns
`Option<(Connection<I>, Bytes)>`.
* `Endpoint::try_send_message` and
`Endpoint::try_send_message_with` have been removed. Use
`Endpoint::get_connection_by_addr` and `Connection::send_*` instead.

* remove `Endpoint::try_send_*` ([3e1bff4](https://github.com/maidsafe/qp2p/commit/3e1bff4565a4ff268634b8a96ef17f9fd139cb1e))
* yield `Connection`s from `Incoming*` streams ([60dd829](https://github.com/maidsafe/qp2p/commit/60dd8298b53f79e602378f5d4c1123540291b534))

## [0.21.0](https://github.com/maidsafe/qp2p/compare/v0.20.0...v0.21.0) (2021-09-24)


### ⚠ BREAKING CHANGES

* `Endpoint::send_message` and
`Endpoint::send_message_with` have been removed. Use
`Endpoint::connect_to` in combination with `Connection::send` or
`Connection::send_with` instead.
* The `Endpoint::send_message_with` and
`Endpoint::try_send_message_with` methods now take `retries` as an
`Option<&RetryConfig>`, rather than `Option<RetryConfig>`.

* move `Endpoint::send_*` to `Connection` ([0f95b8a](https://github.com/maidsafe/qp2p/commit/0f95b8a8137c2e6d5cbc72818fb4322344bcb539))
* use `&RetryConfig` for retry overrides ([70835ed](https://github.com/maidsafe/qp2p/commit/70835edd20967793b5b564122dd6c59297b71257))

## [0.20.0](https://github.com/maidsafe/qp2p/compare/v0.19.0...v0.20.0) (2021-09-21)


### ⚠ BREAKING CHANGES

* `Endpoint::get_connection_id` and
`Endpoint::get_socket_addr_by_id` have been removed.
`Endpoint::get_connection_by_addr` and `Endpoint::get_connection_by_id`
can be used instead to get a `Connection`, from which the `id` or
`remote_address` can be retrieved.
* `Endpoint::new`, `Endpoint::connect_to`, and
`Endpoint::connect_to_any` now use `Connection<I>` instead of
`SocketAddr` in their return type.

### Features

* return `Connection` from `Endpoint::connect_*` ([9ce6947](https://github.com/maidsafe/qp2p/commit/9ce6947f991a18eed665f1766bce6a1b9cdb8448))


* replace `Endpoint` `addr`/`id` getters ([c2ab8a1](https://github.com/maidsafe/qp2p/commit/c2ab8a17b8d77dbdfac62ce254e59782c9087e83))

## [0.19.0](https://github.com/maidsafe/qp2p/compare/v0.18.0...v0.19.0) (2021-09-14)


### ⚠ BREAKING CHANGES

* **retries:** Removal of Eq and PartialEq derivations on config.

### Features

* **config:** nest retry config ([edc5493](https://github.com/maidsafe/qp2p/commit/edc5493294e6ede45ebbd0023be51757c355793d))
* **retries:** extend send msg api with retry cfg ([af6fe59](https://github.com/maidsafe/qp2p/commit/af6fe590abc0d09f6380e6f2bb403700f76440e4))

## [0.18.0](https://github.com/maidsafe/qp2p/compare/v0.17.4...v0.18.0) (2021-09-10)


### ⚠ BREAKING CHANGES

* The `port_forward` field on `Config` is only present
with the `igd` feature enabled (default).
* The `no-igd` feature has been removed.

* Make `Config::port_forward` depend on `igd` feature ([ce3e602](https://github.com/maidsafe/qp2p/commit/ce3e602c7c641aa3cf5c7ffc316d9e203d18e847))
* Remove `no-igd` feature ([b5e2938](https://github.com/maidsafe/qp2p/commit/b5e2938e70cc6d00e97e1da4260bd17f88745ef3))

### [0.17.4](https://github.com/maidsafe/qp2p/compare/v0.17.3...v0.17.4) (2021-09-07)


### Bug Fixes

* Don't report an error on benign end of stream ([d1b4e22](https://github.com/maidsafe/qp2p/commit/d1b4e222b82c4b756823e47f9623c3654c906ef2))

### [0.17.3](https://github.com/maidsafe/qp2p/compare/v0.17.2...v0.17.3) (2021-09-02)

### [0.17.2](https://github.com/maidsafe/qp2p/compare/v0.17.1...v0.17.2) (2021-09-01)


### Features

* derive debug for public channels ([4d262de](https://github.com/maidsafe/qp2p/commit/4d262de3c37937592a120000cf0d466bf310eea7))

### [0.17.1](https://github.com/maidsafe/qp2p/compare/v0.17.0...v0.17.1) (2021-08-31)


### Bug Fixes

* Correctly mark `UpnpError` source ([728d6b4](https://github.com/maidsafe/qp2p/commit/728d6b49d87d7c3f7a45a291ff94c71c3a60bb55))

## [0.17.0](https://github.com/maidsafe/qp2p/compare/v0.16.0...v0.17.0) (2021-08-31)


### ⚠ BREAKING CHANGES

* The signature of `Endpoint::new_client` has changed to
return a tuple of `(Endpoint, IncomingMessages, DisconnectionEvents)`
rather than just the `Endpoint`.

### Features

* Client support for incoming messages and disconnections ([cb945a9](https://github.com/maidsafe/qp2p/commit/cb945a948b11da07f23151b0738c38a5e13a8bab))

## [0.16.0](https://github.com/maidsafe/qp2p/compare/v0.15.3...v0.16.0) (2021-08-27)


### ⚠ BREAKING CHANGES

* The `Endpoint::bootstrap_nodes` method has been
removed since it is no longer set.
* The `Error` type has been removed.
* `Endpoint::try_send_message` now returns `Result<(),
Option<SendError>>` rather than `Result<(), Error>`. The
`Error::MissingConnection` variant has been removed.
* `Endpoint::is_reachable` now returns `Result<(),
RpcError>`, rather than `Result<(), Error>`.
* The `Result` alias has been removed. It can be replaced
by `std::result::Result<T, qp2p::Error>`.
* The `QuicP2p` type has been removed. Use
`Endpoint::new` or `Endpoint::new_client` instead. The
`BootstrapFailure`, `EmptyBootstrapNodesList`, `Io`, `Endpoint`,
`NoEchoServerEndpointDefined`, `EchoServiceFailure`, `CannotAssignPort`,
`IncorrectPublicAddress`, and `UnresolvedPublicIp` `Error` variants have
been removed.
* The `IgdAddPort`, `IgdRenewPort`, `IgdSearch`, and
`IgdNotSupported` variants have been removed from `Error`. These
variants would never have been returned, but may be referenced in
matches against `Error` values.
* The `RecvNextError` and `UnexpectedMessageType` types
have been removed. `RecvError` is used where `RecvNextError` was used
previously.
* The `SendError::TooLong` variant has been removed. The
same condition (attempting to send a message longer than `u32::MAX`
bytes) will now return `SendError::Serialization`.
* `RecvStream::next` now returns `RecvNextError` as the
error type. The following `Error` variants have been removed:

- `Error::Serialisation` (this is now fully covered by `SendError` and
  `RecvError`).
- `Error::InvalidMsgFlag` (this case is now covered by
  `RecvError::Serialization`).
- `Error::StreamRead` (this is now covered by `RecvError`).
- `Error::EmptyResponse` (this case is now covered by
  `RecvError::Serialization`).
- `Error::UnexpectedMessageType` (this case is now covered by
  `ReadNextError::UnexpectedMessageType).

Finally, a `Recv` variant has been added to `Error`.
* The following APIs had their error type changed from
`Error` to `SendError`:

- `SendStream::send_user_msg`
- `SendStream::send`
- `SendStream::finish`
- `Endpoint::send_message`

Additionally, the `StreamWrite` and `MaxLengthExceeded` variants have
been removed from `Error`, and a `Send` variant has been added.
* `SendStream::send` is no longer public.
`Error::UnexpectedMessageType` now contains a simple error struct.
* `Endpoint::connect_to` and
`Endpoint::open_bidirectional_stream` now use `ConnectionError`, rather
than `Error`, for error results.
* `Endpoint::disconnect_from` now returns `()`.
* - The `Error::UnknownStream` variant has been removed.
- `SendStream::set_priority` is now infallible and returns `()`.
- `SendStream::priority` has been removed.
* The `Error::Connect` and `Error::ConnectionClosed`
variants have been removed. Several additional variants have been added
to `ConnectionError`.
* `Endpoint::connect_to_any` now returns
`Option<SocketAddr>` instead of `Result<SocketAddr>`.
* The `ConnId::generate` method now returns `Self`, and
doesn't allow for errors. The `Error::ConnectionIdGeneration` variant
has been removed.
* `Endpoint::socket_addr` has been renamed to
`public_addr`.
* The `Error::UnexpectedError` variant has been removed.
* The `Error::DisconnectionNotification` and
`Error::Configuration` variants have been removed.
* There are several breaking changes:

- `QuicP2p::from_config` now returns `Result<Self, ConfigError>`.
- The `Error::CertificateParse` variant has been removed.
- The `Error::CertificatePkParse` variant has been removed.
- The `Error::CertificateGen` variant has been removed.
- The `Error::Tls` variant has been removed.
* The `Error::Base64Decode` variant has been removed.
* The duration fields in config have all changed:

- `idle_timeout_msec` is renamed `idle_timeout` and is now an
  `Option<Duration>`.
- `keep_alive_interval_msec` is renamed `keep_alive_interval` and is now
  an `Option<Duration>`.
- `upnp_lease_duration` is now an `Option<Duration>`.
- `retry_duration_msec` is renamed `min_retry_duration` and is now an
  `Option<Duration>`.
* `QuicP2p::from_config` now takes a `Config` argument,
rather than `Option<Config>`. `Config::default()` (or
`Option::unwrap_or_default`) should be used instead.
* `Config::hard_coded_contacts` and
`Config::bootstrap_cache_dir` have been removed. Additionally,
`QuicP2p::with_config` no longer takes `bootstrap_nodes` – these should
instead be passed to `QuicP2p::bootstrap`. Finally, the
`Error::InvalidPath` and `Error::UserHomeDir` variants have been
removed.
* `Config::local_ip` and `Config::local_port` have been
removed. `QuicP2p::bootstrap` and `QuicP2p::new_endpoint` now require a
local address to bind to. The `Error::UnspecifiedLocalIp` variant has
been removed.
* This is a breaking behavioural change. It is now up to
the caller if they want to retry with a different port.
* `Config::max_msg_size_allowed` has been removed.

### Features

* Add `Endpoint::connect_to_any` ([59d0b5e](https://github.com/maidsafe/qp2p/commit/59d0b5e4815319369b31dd6c5307101292155adb))
* Add `Endpoint::new_client` constructor ([176d025](https://github.com/maidsafe/qp2p/commit/176d0257536f062a2743bd344e7803ce88c5dce8))
* Add `RecvNextError` for receive-specific errors ([7e6e4a9](https://github.com/maidsafe/qp2p/commit/7e6e4a97d76e164f82fa96af9896a0fa6e38710a))
* Add `SendError` for send-specific errors ([73c6501](https://github.com/maidsafe/qp2p/commit/73c65010fc6bd59493c197c4c8c8a509677e644a))
* Add public `Endpoint::new` constructor ([c448df2](https://github.com/maidsafe/qp2p/commit/c448df23db749c4ab8de309df050df020278d1fd))


### Bug Fixes

* Add missing re-export for `TransportErrorCode` ([8cf4f88](https://github.com/maidsafe/qp2p/commit/8cf4f88cfa85a7628d731faa53e30491edfeee67))
* Don't use connection pool in `Endpoint::is_reachable` ([8a7260b](https://github.com/maidsafe/qp2p/commit/8a7260b5063a21cb0fd06b63e3ffb29d663061bd))
* Fix `WireMsg` visibility ([33e9b3b](https://github.com/maidsafe/qp2p/commit/33e9b3b78f9f8eaedc16af1fbac7c5c624c8e1fe))
* Make connection deduplicator handle cancellations ([a0404ac](https://github.com/maidsafe/qp2p/commit/a0404ac8ba8a46c0741609558655478ab0634d32))


* Delete `SerializableCertificate` ([4bbcd29](https://github.com/maidsafe/qp2p/commit/4bbcd29f381e412e51ede5c439e978a937044adf))
* Make `Config` in `QuicP2p::from_config` non-optional ([19b3795](https://github.com/maidsafe/qp2p/commit/19b379595d25564bd8eb71dba20e50e803f12b02))
* Make `ConnId::generate` infallible ([ea089ac](https://github.com/maidsafe/qp2p/commit/ea089acc7472b2c3e93dfe5dcaba7e9ac10053c7))
* Make `Endpoint::disconnect_from` infallible ([9eb947f](https://github.com/maidsafe/qp2p/commit/9eb947fe3b600af4e0a8744768e0ba9c94122b49))
* Merge 'connect' and 'close' errors with `ConnectionError` ([9aae2e3](https://github.com/maidsafe/qp2p/commit/9aae2e3758302410ca3a2142bc164c96f81bc1c4))
* Move local address from config to `QuicP2p::bootstrap` ([de9763d](https://github.com/maidsafe/qp2p/commit/de9763d811a95f2c38d4e00677400b53bc4286d8))
* Refactor `WireMsg::read_from_stream` to avoid logic error ([047d071](https://github.com/maidsafe/qp2p/commit/047d0711ecce339d14faade7d85f8cf2ea8063bf))
* Remove `BootstrapCache` and hard-coded contacts ([df3fef8](https://github.com/maidsafe/qp2p/commit/df3fef827eb68d019cc713eb7cd003ec477715cd))
* Remove `Endpoint::bootstrap_nodes` ([c399bca](https://github.com/maidsafe/qp2p/commit/c399bca3f5e6469b71471a292e435af168e15096))
* Remove `Error::UnknownStream` variant ([2386486](https://github.com/maidsafe/qp2p/commit/2386486ae156add6ccbe5036aaedea3779e9d07b))
* Remove `Error` ([5608f27](https://github.com/maidsafe/qp2p/commit/5608f27634439828eab22b53a2fb858b60d7249f))
* Remove `QuicP2p` ([44d964c](https://github.com/maidsafe/qp2p/commit/44d964c6f3723eaf6899e84dc967a3bb457b56f3))
* Remove `Result` alias ([e528134](https://github.com/maidsafe/qp2p/commit/e52813455e942432254f501da95cfcf7571146e5))
* Remove random port fallback ([9b1612a](https://github.com/maidsafe/qp2p/commit/9b1612a248162eefbf08fcf22cdbe2c702066002))
* Remove unused `Config::max_msg_size_allowed` ([44d750f](https://github.com/maidsafe/qp2p/commit/44d750fbcd6bf68d0cf07b4a6edb90d23dc63a59))
* Remove unused `Error` variants ([3b8174a](https://github.com/maidsafe/qp2p/commit/3b8174ae7f85d62d27f69f7405f35e00b2feb1c1))
* Rename `Endpoint::socket_addr` to `public_addr` ([91d8457](https://github.com/maidsafe/qp2p/commit/91d8457b50f165ab8641eab3145614bba9e1f74a))
* Return `Option` from `Endpoint::connect_to_any` ([5d374fa](https://github.com/maidsafe/qp2p/commit/5d374fa6010ffc585fe79bcd87e1743fcf1bc40a))
* Return `RpcError` from `Endpoint::is_reachable` ([674ad2d](https://github.com/maidsafe/qp2p/commit/674ad2d4df9a2b29cb9267f03524fb99b4ff49cf))
* Return `SendError` in `Endpoint::try_send_message` ([c43855b](https://github.com/maidsafe/qp2p/commit/c43855bc57265216731a6a2ed7f0289486504b02))
* Separate `ConfigError` from `Error` ([e837aca](https://github.com/maidsafe/qp2p/commit/e837aca1981553624ecd82133c93706b08741b35))
* Treat unexpected messages as serialization errors ([e0695c3](https://github.com/maidsafe/qp2p/commit/e0695c3116bed6f830939a8eafbe045a2de39b61))
* Use `ConnectionError` when possible ([db74509](https://github.com/maidsafe/qp2p/commit/db74509cb456c3930fd0eee4b67687a9291e4b00))
* Use `Duration` in config and centralise defaults ([86f886b](https://github.com/maidsafe/qp2p/commit/86f886bb3f0a582e4db83b9fff26ba6fddc00a7f))
* Use `SerializationError` for too long messages ([5bf79c1](https://github.com/maidsafe/qp2p/commit/5bf79c178491bbc1a5aa1412794601303cb2807a))
* Use a separate type for IGD errors ([d9413de](https://github.com/maidsafe/qp2p/commit/d9413deec367be3d6be332d5200d719fe8ba878c))

### [0.15.3](https://github.com/maidsafe/qp2p/compare/v0.15.2...v0.15.3) (2021-08-24)

### [0.15.2](https://github.com/maidsafe/qp2p/compare/v0.15.1...v0.15.2) (2021-08-11)


### Features

* **priority:** allow setting priority of a stream ([a0cf893](https://github.com/maidsafe/qp2p/commit/a0cf8938d02b9a7644c70951e6a9c8ab24339c82))

### [0.15.1](https://github.com/maidsafe/qp2p/compare/v0.15.0...v0.15.1) (2021-08-11)

## [0.15.0](https://github.com/maidsafe/qp2p/compare/v0.14.1...v0.15.0) (2021-08-11)


### ⚠ BREAKING CHANGES

* The `Error::QuinnConnectionClosed` variant has been
renamed `Error::ConnectionClosed` and now contains closure information.
The `Error::Connection` variant has been renamed
`Error::ConnectionError`, and now contains a `ConnectionError`.
`quinn::ConnectionError` is no longer re-exported.

* Separate connection close from errors ([4d3d4eb](https://github.com/maidsafe/qp2p/commit/4d3d4eb3a747dff159d7b8239da4f3adabd17768))

### [0.14.1](https://github.com/maidsafe/qp2p/compare/v0.14.0...v0.14.1) (2021-08-11)

## [0.14.0](https://github.com/maidsafe/qp2p/compare/v0.13.0...v0.14.0) (2021-08-03)


### ⚠ BREAKING CHANGES

* **quinn:** config change

### Bug Fixes

* **ci:** ignore quinn test on CI ([5f7b9f0](https://github.com/maidsafe/qp2p/commit/5f7b9f0e51c94a87aa3dbc0a1462c6c91e8fd3e0))
* **clippy:** update to use latest version of rust ([18d48c4](https://github.com/maidsafe/qp2p/commit/18d48c4ac449a9927667287e31e17bcf759f6283))
* **disconnections:** remove connection from pool before signalling disconnection event ([d45204c](https://github.com/maidsafe/qp2p/commit/d45204c3b854802bdd4b4d028a230e9c6b93caac))
* **quinn:** disable stateless retry in quinn config ([0f5df36](https://github.com/maidsafe/qp2p/commit/0f5df36d2f9ed77660c05cd0b4860f34b36502a7))

## [0.13.0](https://github.com/maidsafe/qp2p/compare/v0.12.11...v0.13.0) (2021-08-03)


### ⚠ BREAKING CHANGES

* **id:** introduces generic type to Qp2p and Endpoint

### Features

* **id:** keep a track of connections using a connection ID within qp2p ([10c8ea0](https://github.com/maidsafe/qp2p/commit/10c8ea036305f3b71dd2acbb2712ad2e104ddfd7))


### Bug Fixes

* **api:** rename type parameter after rebase ([8b2ad02](https://github.com/maidsafe/qp2p/commit/8b2ad026438a003536f39d83ddb8a55cac2564a8))

### [0.12.11](https://github.com/maidsafe/qp2p/compare/v0.12.10...v0.12.11) (2021-07-28)


### Features

* Make total backoff duration configurable ([75d26ea](https://github.com/maidsafe/qp2p/commit/75d26ea2b8336fdf5a50cd758422e0b402737f8f))
* retry connection attempts for new connections too ([fef624d](https://github.com/maidsafe/qp2p/commit/fef624de060e10353e7ec6f7006d660696bd4e63))
* use backoff w/ jitter for retries ([fec5d07](https://github.com/maidsafe/qp2p/commit/fec5d07a7204e48d0957c9c745264dcbf4e94818))

### [0.12.10](https://github.com/maidsafe/qp2p/compare/v0.12.9...v0.12.10) (2021-07-15)


### Features

* Add config option for retry interval ([6bba5ec](https://github.com/maidsafe/qp2p/commit/6bba5ec176363490f3ea7f60be2e1a9bf9df63fa))
* retry on connect, retry on send ([94bfb8b](https://github.com/maidsafe/qp2p/commit/94bfb8b84e87bb536e4288798c7330a8bd69b60f))

### [0.12.9](https://github.com/maidsafe/qp2p/compare/v0.12.8...v0.12.9) (2021-07-13)


### Bug Fixes

* increase channel size ([fa581ae](https://github.com/maidsafe/qp2p/commit/fa581ae1d7445bf6e09f827b3b68b556f057aa7d))

### [0.12.8](https://github.com/maidsafe/qp2p/compare/v0.12.7...v0.12.8) (2021-07-13)

### [0.12.7](https://github.com/maidsafe/qp2p/compare/v0.12.6...v0.12.7) (2021-07-12)


### Features

* use tracing for logging ([d65285b](https://github.com/maidsafe/qp2p/commit/d65285b0d42673c8d11c4ff3b6a5d19b4a46e029))

### [0.12.6](https://github.com/maidsafe/qp2p/compare/v0.12.5...v0.12.6) (2021-07-02)


### Bug Fixes

* **igd:** Fix the condition for terminating the IGD renewal loop ([295626c](https://github.com/maidsafe/qp2p/commit/295626c7dc326ec880124df49bcb1571d7a86b9e))

### [0.12.5](https://github.com/maidsafe/qp2p/compare/v0.12.4...v0.12.5) (2021-07-01)


### Features

* **error:** add error variant when creating port binding fails ([24b2bc5](https://github.com/maidsafe/qp2p/commit/24b2bc5d6612f3ea4945d11afb80ab35f83c93a1))


### Bug Fixes

* **clippy:** fix clippy issues ([53de7a9](https://github.com/maidsafe/qp2p/commit/53de7a946de283660bf8e0ad29f43ae22b6233f4))
* **igd:** stop renewing port mapping after endpoint.close() is called ([949b372](https://github.com/maidsafe/qp2p/commit/949b3725d4aedbc9d0cea5136ade8eb9ed6b5ab6))

### [0.12.4](https://github.com/maidsafe/qp2p/compare/v0.12.3...v0.12.4) (2021-06-24)

### [0.12.3](https://github.com/maidsafe/qp2p/compare/v0.12.2...v0.12.3) (2021-06-14)

### [0.12.2](https://github.com/maidsafe/qp2p/compare/v0.12.1...v0.12.2) (2021-06-14)

### [0.12.1](https://github.com/maidsafe/qp2p/compare/v0.12.0...v0.12.1) (2021-06-09)


### Bug Fixes

* avoid re-connect attempts on existing connection ([3cde437](https://github.com/maidsafe/qp2p/commit/3cde43768d1df1cd54c1f2659d8abccc89e99038))

## [0.12.0](https://github.com/maidsafe/qp2p/compare/v0.11.11...v0.12.0) (2021-06-07)


### ⚠ BREAKING CHANGES

* **refactor:** updates some apis to be async

### Features

* **refactor:** remove unnecessary mutex ([8066b18](https://github.com/maidsafe/qp2p/commit/8066b18a13ebbf07cf5f3304435a47afab5a5600))


### Bug Fixes

* **tests:** minor test fixes ([95ab0f7](https://github.com/maidsafe/qp2p/commit/95ab0f74f13ebd3ef5cad06a3178dfd909f28f48))

### [0.11.11](https://github.com/maidsafe/qp2p/compare/v0.11.10...v0.11.11) (2021-06-01)

### [0.11.10](https://github.com/maidsafe/qp2p/compare/v0.11.9...v0.11.10) (2021-05-25)


### Features

* **api:** create new try_send_message API and change send_message ([1264246](https://github.com/maidsafe/qp2p/commit/12642469b55350d0eaeeeb2da8d3320b5682d5f3))

### [0.11.9](https://github.com/maidsafe/qp2p/compare/v0.11.8...v0.11.9) (2021-04-26)


### Features

* **api:** add public API to get an endpoint's list of bootstrap nodes ([075c89e](https://github.com/maidsafe/qp2p/commit/075c89e30adc19cea0d60d76182a581ea3b2cdcb))
* **boostrap_cache:** add API to replace contents of bootstrap cache ([0684769](https://github.com/maidsafe/qp2p/commit/0684769d68ddfc5cfb5905e0ea538c792bec5412))
* **endpoint:** add functionality for re-boostrapping to the network ([af6b54d](https://github.com/maidsafe/qp2p/commit/af6b54d33fdc766c504bb313dfa600cc9a4af240))

### [0.11.8](https://github.com/maidsafe/qp2p/compare/v0.11.7...v0.11.8) (2021-04-13)


### Features

* **api:** add new api to check if endpoint is externally reachable ([6305442](https://github.com/maidsafe/qp2p/commit/63054421b7514967f41843111cae068398aca784))


### Bug Fixes

* **igd:** run igd even if echo service succeedes ([b2a2971](https://github.com/maidsafe/qp2p/commit/b2a297135ecd40d715dec615c757bee19f752c4b))

### [0.11.7](https://github.com/maidsafe/qp2p/compare/v0.11.6...v0.11.7) (2021-04-08)


### Bug Fixes

* **boostrap:** connect to peers concurrently when querying their echo service ([bb6b7dd](https://github.com/maidsafe/qp2p/commit/bb6b7dd149ecfa7ced30eda1a6e3640a3e0206b5))

### [0.11.6](https://github.com/maidsafe/qp2p/compare/v0.11.5...v0.11.6) (2021-04-07)


### Bug Fixes

* **bootstrap:** fix stalled connections w/multiple bootstrap contacts ([cd02b6a](https://github.com/maidsafe/qp2p/commit/cd02b6a5d8819ac96b997b9271c712b2f679be8a))

### [0.11.5](https://github.com/maidsafe/qp2p/compare/v0.11.4...v0.11.5) (2021-04-06)


### Bug Fixes

* **endpoint_verification:** add timeout for endpoint verification query ([2fc7041](https://github.com/maidsafe/qp2p/commit/2fc70418d839e2b4209f1ca06864a1edcd2ef3b8))

### [0.11.4](https://github.com/maidsafe/qp2p/compare/v0.11.3...v0.11.4) (2021-04-05)

### [0.11.3](https://github.com/maidsafe/qp2p/compare/v0.11.2...v0.11.3) (2021-04-02)


### Bug Fixes

* **manual-port-forwarding:** use the existing endpoint to validate ([56e6626](https://github.com/maidsafe/qp2p/commit/56e66263527aeef5d3cbf49e361b5a19bb80bf28))

### [0.11.2](https://github.com/maidsafe/qp2p/compare/v0.11.1...v0.11.2) (2021-03-29)


### Bug Fixes

* **clippy:** rename error variant to fix clippy issue with latest rust ([c2d0227](https://github.com/maidsafe/qp2p/commit/c2d0227bc4179a264dbf1aac16448b018f16dd06))
* **config:** remove duplicate options for config params ([b40f2ef](https://github.com/maidsafe/qp2p/commit/b40f2ef7fc8ef217536b29510a42988f66b1460f))
* remove duplicate short option ([30fd110](https://github.com/maidsafe/qp2p/commit/30fd110d7083041888fa4b3712cd1782efec8deb))

### [0.11.1](https://github.com/maidsafe/qp2p/compare/v0.11.0...v0.11.1) (2021-03-28)


### Features

* add peer address to connection log messages ([1130207](https://github.com/maidsafe/qp2p/commit/113020747804eb4a94172dbba9162e9e66a84ce2))

## [0.11.0](https://github.com/maidsafe/qp2p/compare/v0.10.1...v0.11.0) (2021-03-25)


### ⚠ BREAKING CHANGES

* **header:** this changes the data_len field in MsgHeader from usize
to u32

* **header:** Allow compilation on 32 bit architectures ([7c39399](https://github.com/maidsafe/qp2p/commit/7c39399dbc4596e5497cd0b8ef11ab5532d63529))

### [0.10.1](https://github.com/maidsafe/qp2p/compare/v0.10.0...v0.10.1) (2021-03-11)


### Bug Fixes

* read multiple messages from a single stream ([949fc4b](https://github.com/maidsafe/qp2p/commit/949fc4b49b0e3c2d4b6afb96b58a41166e4e640f))

## [0.10.0](https://github.com/maidsafe/qp2p/compare/v0.9.25...v0.10.0) (2021-03-05)


### ⚠ BREAKING CHANGES

* **tokio:** new Tokio v1 is not backward compatible with previous runtime versions < 1.

* **tokio:** upgrade tokio to v1.2.0, quinn to v0.7.0 and rustls to v0.19.0 ([0465cf8](https://github.com/maidsafe/qp2p/commit/0465cf87c04e860fff3532bd6133c0497ced0c6d))

### [0.9.25](https://github.com/maidsafe/qp2p/compare/v0.9.24...v0.9.25) (2021-03-03)

### [0.9.24](https://github.com/maidsafe/qp2p/compare/v0.9.23...v0.9.24) (2021-02-25)

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
