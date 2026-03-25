# Changelog

All notable changes to BillPouch will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.1](https://github.com/Onheiron/BillPouch/compare/billpouch-v0.3.0...billpouch-v0.3.1) (2026-03-25)


### Features

* **api:** aggiungi route pause/resume/farewell-evict/invite a bp-api ([9596bfd](https://github.com/Onheiron/BillPouch/commit/9596bfd2063f3be8859537e206720a0e1527b026))
* **api:** web dashboard — embedded SPA served at GET / ([d4cfa3b](https://github.com/Onheiron/BillPouch/commit/d4cfa3b0ee610d4b01189fd3e01272071759a683))
* bp flock mostra tier per servizi Pouch, fix hint join obsoleta ([59fae24](https://github.com/Onheiron/BillPouch/commit/59fae2429612312e0fdcf9a27d2b527d28a00c3e))
* bp-api — REST API gateway Axum (9 endpoint, proxy su Unix socket) ([6e5ca17](https://github.com/Onheiron/BillPouch/commit/6e5ca17ed9d8d4ac275a22b737ccad741ef26550))
* CEK hints persistence (cek_hints.json) — files survive daemon restart ([3fbf8fd](https://github.com/Onheiron/BillPouch/commit/3fbf8fd4242e767cb126b78044a5535ff799c720))
* **cli:** aggiunge comando bp status — identita daemon e servizi in formato compatto ([48242c7](https://github.com/Onheiron/BillPouch/commit/48242c7f076fc1e005c71fde9d906ded00a438a0))
* **cli:** bp bootstrap list/add/remove — gestione WAN bootstrap nodes ([d709864](https://github.com/Onheiron/BillPouch/commit/d709864001ee49686f886d9bf39ba921461215c5))
* **cli:** bp farewell --evict + StorageManager::purge() + ControlRequest::FarewellEvict ([906746e](https://github.com/Onheiron/BillPouch/commit/906746ef020e462a3cdc8b861704c3100d8cc8cd))
* **cli:** bp hatch pouch --tier T1-T5, one-Pouch-per-network enforcement ([9ff1b32](https://github.com/Onheiron/BillPouch/commit/9ff1b32cced18653028c5fa40d02c852361f9f98))
* **cli:** bp leave --force auto-evict — protocol, server, CLI, wiki aggiornati ([3ab7de3](https://github.com/Onheiron/BillPouch/commit/3ab7de3138703197ebf5a6c174ab07ba8a7931ac))
* **cli:** bp leave with service precondition check + Leave handler upgrade ([fad87a6](https://github.com/Onheiron/BillPouch/commit/fad87a6e466da261213fd80b1526653ae6f45cc5))
* **cli:** bp pause --eta / bp resume + ServiceStatus::Paused + ControlRequest::{Pause,Resume} ([b229c7e](https://github.com/Onheiron/BillPouch/commit/b229c7eec3529bd538412737b0c81bdd2b18a2ce))
* **core:** adaptive k/n in PutFile via QoS + update protocol and CLI ([616323e](https://github.com/Onheiron/BillPouch/commit/616323ea3112cd9effa7312c09e5c6eaee9d076b))
* **core:** add QoS tracking, adaptive k params, and file manifest ([1e49a7f](https://github.com/Onheiron/BillPouch/commit/1e49a7f74cc053463adda57975fdd21ce950c3f2))
* **core:** add RLNC erasure coding (GF(2^8), encode/recode/decode) and StorageManager ([77dbf6a](https://github.com/Onheiron/BillPouch/commit/77dbf6aaf9472cf851e5bee9e876b0e3244f6ac3))
* **core:** Bootstrap nodes — scoperta WAN via bootstrap.json ([16ff6ed](https://github.com/Onheiron/BillPouch/commit/16ff6edc87d3e6a4a1b6e982f18728154b84383d))
* **core:** FragmentIndex gossip — RemoteFragmentIndex, AnnounceIndex, targeted GetFile fetch ([5bf7c86](https://github.com/Onheiron/BillPouch/commit/5bf7c8620a6ec04e52526ec8189131a0615038fe))
* **core:** network quality monitor — Ping/Pong RTT loop ([dc06e5e](https://github.com/Onheiron/BillPouch/commit/dc06e5ec8f8e597ee74f124aa63d0ab47fe8ca78))
* **core:** Persistenza Kademlia — KadPeers save/load su disco ([9ffa920](https://github.com/Onheiron/BillPouch/commit/9ffa9208d3936783d7dd7fa5b6eb47936974bb82))
* **core:** Proof-of-Storage challenge — fault score, OutgoingAssignments, PoS loop ([7461367](https://github.com/Onheiron/BillPouch/commit/746136780c9b437040892ffcc2e9f417239cb327))
* **core:** Rigenerazione Preventiva — rerouting frammenti quando fault_score &gt;= FAULT_SUSPECTED ([599aade](https://github.com/Onheiron/BillPouch/commit/599aade5378566208977f22d5f5e2079083fefa4))
* **identity:** multi-device export/import — bp export-identity / bp import-identity ([487a26d](https://github.com/Onheiron/BillPouch/commit/487a26de9163a38ebe376220a931f8ca352205e8))
* **marketplace:** storage marketplace — offers, acceptances, agreements ([b9fd5a7](https://github.com/Onheiron/BillPouch/commit/b9fd5a79aee25679b7d8675b1bd1166ad6754174))
* **nat:** AutoNAT + relay client for NAT traversal ([90cec5e](https://github.com/Onheiron/BillPouch/commit/90cec5e179ca6b0363dc5abb3ea8d273f4c725ea))
* **network:** ReputationTier R0-R4 + ReputationStore + DaemonState integration ([951f5ad](https://github.com/Onheiron/BillPouch/commit/951f5adaeeba3512c21fc2cf0677263cc9240b42))
* P2P fragment distribution and remote collection ([b8d1fb8](https://github.com/Onheiron/BillPouch/commit/b8d1fb813c546609cf1934ef369695dcda4da456))
* **playground:** add interactive local P2P network simulator ([40db85a](https://github.com/Onheiron/BillPouch/commit/40db85ac9d57aa25a60f06044bd6f58f6065bfed))
* PutFile/GetFile, StorageManager integration, fragment exchange, bp put/get CLI ([116c3ab](https://github.com/Onheiron/BillPouch/commit/116c3aba1d79eef97774848b8bf8dcd006ea6969))
* **security:** CEK per-user encryption + NetworkMetaKey as random secret ([23780cb](https://github.com/Onheiron/BillPouch/commit/23780cb32f5ab78ff776a01b6ef714be41e69908))
* **security:** encryption at rest — ChaCha20-Poly1305 per chunk before RLNC ([b0ada4a](https://github.com/Onheiron/BillPouch/commit/b0ada4aef3e5c04b5f5797d84051ea084a538a10))
* **security:** invite system — signed+encrypted network invite tokens ([6be3981](https://github.com/Onheiron/BillPouch/commit/6be398192062296c0440d59367578a8387ac8951))
* **security:** passphrase-protected identity (Argon2id + ChaCha20-Poly1305) ([f4045c5](https://github.com/Onheiron/BillPouch/commit/f4045c53d97a001541bff034f9a652a8517ccd8f))
* **smoke:** add Docker Compose smoke test with 3 P2P nodes ([2ea2ab8](https://github.com/Onheiron/BillPouch/commit/2ea2ab85c57d72afb96a7b26948257c247cbc79d))
* **storage:** StorageTier T1-T5 + BpError::InvalidInput ([52830c8](https://github.com/Onheiron/BillPouch/commit/52830c8f1e53ec2c12e9f9be1c98bb8fdc456d08))


### Bug Fixes

* aggiunge force:false a tutti i literal ControlRequest::Leave nei test e in bp-api ([a5cf17b](https://github.com/Onheiron/BillPouch/commit/a5cf17bd9bf67f9e34520271385c3aa9f0ac9fae))
* **api:** corregge CreateInvite fields, rewrite redeem senza ControlRequest inesistente ([6113f53](https://github.com/Onheiron/BillPouch/commit/6113f530f49705e11e630a87fc91a88d3ab91b03))
* **ci:** accept any swarm build failure on CI environments ([71f2569](https://github.com/Onheiron/BillPouch/commit/71f2569fa81ff4f2698fa641909463d030758ae1))
* **ci:** coverage - mkdir output dir, remove || true, fail_ci_if_error true ([4023beb](https://github.com/Onheiron/BillPouch/commit/4023bebfdfc3563e9b18d24fadc3b476e9b22edf))
* **ci:** fix failing GitHub Actions workflows ([1d84dba](https://github.com/Onheiron/BillPouch/commit/1d84dbafbd7a2bb1cf3742415e912cb018a5681a))
* **ci:** fix failing GitHub Actions workflows ([332b854](https://github.com/Onheiron/BillPouch/commit/332b854683c1510b1e061f9996fd621566538313))
* **ci:** fix formatting, release-please, and coverage workflows ([430bb64](https://github.com/Onheiron/BillPouch/commit/430bb640e404e08c4bacf5b96663e72b75b4aa3b))
* **ci:** propagate mDNS errors instead of panicking in build_swarm ([d5d89bb](https://github.com/Onheiron/BillPouch/commit/d5d89bb3c5c086da04eb63e1abb0221dc37da570))
* **ci:** remove invalid --all-features flag from cargo deny check ([65b2999](https://github.com/Onheiron/BillPouch/commit/65b299997a5db669dc92e7ddb4a7befd93a0d640))
* **ci:** security jobs red+continue-on-error at job level, coverage back to architecture_test ([576e6ba](https://github.com/Onheiron/BillPouch/commit/576e6ba8a2f240ac411fff90e661b9e6eb2883b4))
* **ci:** security jobs show yellow warning on step, not green, with step summary ([e4d0b4f](https://github.com/Onheiron/BillPouch/commit/e4d0b4fe11ce34e13690bf416ce30bb1166959ee))
* **ci:** use #[tokio::test] for swarm test to provide Tokio runtime on Linux ([c2cc481](https://github.com/Onheiron/BillPouch/commit/c2cc481499a19ef2cb468fc292ece2d3615522be))
* **cli:** add clap env feature for BP_PASSPHRASE env var support ([214a1fd](https://github.com/Onheiron/BillPouch/commit/214a1fdeaa121bf25cbacf256a14fe99ac4e4c60))
* **clippy:** add .. to ReservationReqAccepted pattern (renewal+limit fields) ([2439865](https://github.com/Onheiron/BillPouch/commit/2439865bf6b7baaae297915a30591fea79fa0ce9))
* **clippy:** allow too_many_arguments on handle_swarm_event ([bd294f7](https://github.com/Onheiron/BillPouch/commit/bd294f7105f557abc7beeba9c859e003d03286ce))
* **clippy:** allow too_many_arguments on run_network_loop ([2d5b3ee](https://github.com/Onheiron/BillPouch/commit/2d5b3eea743a0ec83ebbc51a10b315a788a57c4e))
* **clippy:** clone_on_copy — PeerId is Copy, use *peer_id ([031b85b](https://github.com/Onheiron/BillPouch/commit/031b85bd17921c8f44af0fb9f7f495df3c1a216f))
* **clippy:** introduce StorageManagerMap type alias to reduce type complexity ([e039109](https://github.com/Onheiron/BillPouch/commit/e0391091a79a7839673621f3f8fbe01bc4a27faa))
* **clippy:** remove unnecessary libp2p:: qualification for Multiaddr in server.rs ([1ae4a44](https://github.com/Onheiron/BillPouch/commit/1ae4a4457a29db826e69523071b57006144044b8))
* **clippy:** remove unused PeerQos import in quality_monitor tests ([7d199a1](https://github.com/Onheiron/BillPouch/commit/7d199a1e22837d43439d9d95c333f6af1945f393))
* **clippy:** remove unused PutFileResponse struct and Serialize import ([678007e](https://github.com/Onheiron/BillPouch/commit/678007e78e9f8a983e4db76047607ffff616ebb7))
* **clippy:** replace needless range loops with iter_mut/zip in Gaussian elimination ([3f8ee9d](https://github.com/Onheiron/BillPouch/commit/3f8ee9df1bf293863050f079f079afa39bf73241))
* **cli:** remove redundant trailing semicolon in Login dispatch ([871bfc2](https://github.com/Onheiron/BillPouch/commit/871bfc207905aaf321ff663fa4d3732899218d39))
* **cli:** use client.request() instead of .call() in relay connect + explicit type annotation ([1b02520](https://github.com/Onheiron/BillPouch/commit/1b025205cf61ee72a6ac2e47ef38b56ee224d854))
* **core:** add use&lt;'a, '_&gt; bound to pointers_for_peer return type ([b3c7637](https://github.com/Onheiron/BillPouch/commit/b3c763773e6c0c946c883f3bd67670013bb89e62))
* **core:** borrow &all_fragments in rlnc::decode call ([7dcb05c](https://github.com/Onheiron/BillPouch/commit/7dcb05c80b2a43e752c676644ed8e016c844effd))
* **core:** clone metadata before move into ServiceInfo::new ([cd2d3c9](https://github.com/Onheiron/BillPouch/commit/cd2d3c9e5427dc46aad4978af995f2c5ce3a3dc4))
* **core:** fix clippy warnings in params.rs (unused var, excessive precision) ([6c691bc](https://github.com/Onheiron/BillPouch/commit/6c691bc2dc2a07dc9f0d32519b67fad79434be45))
* **core:** fix reversed Horner coefficients in erfc_approx ([a78636c](https://github.com/Onheiron/BillPouch/commit/a78636c7a6104569fdee435bebc7279c32b1a8e8))
* **core:** name lifetimes explicitly in pointers_for_peer to fix use&lt;&gt; capture ([bd9d27b](https://github.com/Onheiron/BillPouch/commit/bd9d27bf1f86a2c0a8a9d757e1d04042bbe4f261))
* **core:** use&lt;'a, '_, '_&gt; for two anonymous lifetimes in pointers_for_peer ([5c775fa](https://github.com/Onheiron/BillPouch/commit/5c775fa0c2ef99b5d856a1c886bc7bb3a3955ffa))
* **dashboard:** aggiorna index.html a v0.3 (local_services, status, rimuovi marketplace) ([5d14e64](https://github.com/Onheiron/BillPouch/commit/5d14e64dcb4346ce7a22c11da43c9c7fed1ff9f4))
* **docker:** bump Rust image to 1.85 for edition2024 support ([d752b52](https://github.com/Onheiron/BillPouch/commit/d752b52ae0bd4b133d3444e8d644057e141f9c50))
* **docs:** escape &lt;PeerId&gt; HTML tag in bootstrap doc comment ([28bcfb1](https://github.com/Onheiron/BillPouch/commit/28bcfb1c3da08138ce809cc3a8905f85d7b8b5b8))
* **docs:** fix rustdoc private/broken intra-doc links in quality_monitor ([c418cdc](https://github.com/Onheiron/BillPouch/commit/c418cdc5dd64293d29eb3b9722659b2f57e82134))
* **docs:** remove broken intra-doc link to DaemonState in reputation.rs ([ac38591](https://github.com/Onheiron/BillPouch/commit/ac385910b144e55f14e2e3ef78f8ded07a2c9fa4))
* **docs:** remove private intra-doc links in qos.rs ([7a9dee4](https://github.com/Onheiron/BillPouch/commit/7a9dee46a466d65addf0a4cca96a87fb2f111f78))
* **docs:** replace broken ReceivedOffers link with AgreementStore ([f5b85cf](https://github.com/Onheiron/BillPouch/commit/f5b85cf775d15b75d97fa2c57396fea196539e32))
* **docs:** replace literal \\n with real newlines in flock.rs doc comment ([ccf1357](https://github.com/Onheiron/BillPouch/commit/ccf135733052f5269928004828e285ddb2f95dff))
* drop renewed field from ReservationReqAccepted pattern (not in libp2p-relay 0.18) ([1d23d51](https://github.com/Onheiron/BillPouch/commit/1d23d51fb8f697777056372d8b53f48b77a8e94d))
* escape curly braces in Mermaid flowchart node labels ([b88fb1a](https://github.com/Onheiron/BillPouch/commit/b88fb1a340f6d53e2e3afda9365e795644ea864e))
* fully remove marketplace handlers from bp-api (leftover from partial edit) ([b029c9d](https://github.com/Onheiron/BillPouch/commit/b029c9deb2e5a47f35f37e4f6b715b1ab4912fa8))
* **invite:** add explicit imports in test module ([782649a](https://github.com/Onheiron/BillPouch/commit/782649a63527c2fe698de610a7d9f4e22463f58c))
* **invite:** remove spurious & in client.send() calls ([a551618](https://github.com/Onheiron/BillPouch/commit/a55161893e3eb89e79935f3cc10c108db99bb09c))
* re-add hidden bp join command to keep gossipsub mesh alive in smoke test ([fb58767](https://github.com/Onheiron/BillPouch/commit/fb587673da6bbd2d1e17e71a905b167917bc8ed0))
* **release:** build binaries in release-please workflow ([9ffe8e2](https://github.com/Onheiron/BillPouch/commit/9ffe8e22111c30349356b4d26e0b9bf3189da75d))
* **release:** trigger on billpouch-v* tags from release-please ([7ad0516](https://github.com/Onheiron/BillPouch/commit/7ad051678f3b8f12d2ccf39203f6d53fe71411a6))
* remove AnnounceOffer variant from NetworkCommand enum ([2d2eca4](https://github.com/Onheiron/BillPouch/commit/2d2eca45c9897b501474ec62f3d54b31fd329475))
* remove bp join from smoke/playground entrypoints (gossip joined at hatch) ([669405a](https://github.com/Onheiron/BillPouch/commit/669405aeb992006ab49828493f27af083468ca04))
* remove spurious & in join.rs send call ([3aebbfb](https://github.com/Onheiron/BillPouch/commit/3aebbfb5fa534d02685a30b42dc0f3d3738e3bc4))
* replace non-ASCII em dash in byte string literal in storage/mod.rs ([9c157be](https://github.com/Onheiron/BillPouch/commit/9c157bec00c19833d4cd36b2e81d4d6ea6b39aa6))
* **rlnc:** guard recode() against all-zero coding vectors (production correctness) ([747e1e7](https://github.com/Onheiron/BillPouch/commit/747e1e702d42c02d7ff4b542a07f60bdf50a17af))
* **rlnc:** systematic encoding + deterministic tests ([3fe12ff](https://github.com/Onheiron/BillPouch/commit/3fe12fff68a00d1297d73fd976f22ff9bba2012a))
* **server:** Leave handler — map(|s| (*s).clone()) per &&ServiceInfo ([b146ed9](https://github.com/Onheiron/BillPouch/commit/b146ed94d2976878a65920b753f1500e1d01e3c9))
* **smoke:** hatch services after mesh formation for gossipsub propagation ([576740c](https://github.com/Onheiron/BillPouch/commit/576740c126957fe7d793e79ec4733bb2b30bd05c))
* **smoke:** join network before mesh wait to keep connections alive ([d068e17](https://github.com/Onheiron/BillPouch/commit/d068e171b4f4ba9513fb30c24144b995ccec0760))
* **smoke:** match service type without brackets in Known Peers section ([38fc272](https://github.com/Onheiron/BillPouch/commit/38fc272ad5b66fd5d1a42a7faafdc666176d10cc))
* **smoke:** move network join into entrypoint for immediate gossipsub subscription ([1d51b83](https://github.com/Onheiron/BillPouch/commit/1d51b83de33239044d3230653aa69e8220e1bd37))
* **smoke:** use safe arithmetic to avoid set -e exit on zero increment ([3045fbd](https://github.com/Onheiron/BillPouch/commit/3045fbde340f55ae2f702e0284a9f3ccbf312db2))
* StatusData.local_services in integration_test (was .services) ([8ec75dc](https://github.com/Onheiron/BillPouch/commit/8ec75dc8a649becf23ef7468e1289190bdac87f4))
* **test+clippy:** add storage_managers to test DaemonState; use clamp() in put.rs ([95e8a89](https://github.com/Onheiron/BillPouch/commit/95e8a89d2cbb1ff767a87c4f499dc9c379b6d2c1))
* **test:** add agreements field to DaemonState in architecture_test ([78eaaa2](https://github.com/Onheiron/BillPouch/commit/78eaaa2836334b437437b949b724711dfe9e86e0))
* **test:** add storage_managers to DaemonState in integration_test.rs ([6a37a44](https://github.com/Onheiron/BillPouch/commit/6a37a44f899d7bc81b4ef3aa8fe05a0256409f3c))
* **test:** config_path test acquires IDENTITY_FS_LOCK to avoid HOME race condition ([854861f](https://github.com/Onheiron/BillPouch/commit/854861f02472074cb86877e54f3ffd6238acc0ff))
* **test:** correggi assert identity_export_con_passphrase -- controlla ciphertext invece di enum Encrypted ([afa7aaf](https://github.com/Onheiron/BillPouch/commit/afa7aafd0c3059e08a5f67ff624df583a514f2ab))
* **test:** persist_cek_hints crea dir, mutex poisoning con unwrap_or_else ([06a6658](https://github.com/Onheiron/BillPouch/commit/06a665814a6b7e718a9cb70f1d8e11d6db40e8f9))
* **test:** recode from full k-rank set to avoid singular matrix in decode_from_recoded_fragments ([343d1b8](https://github.com/Onheiron/BillPouch/commit/343d1b8b7728956a974457c3a1c82349e5aa96f0))
* **tests:** add missing qos field to DaemonState in test helpers ([03642d4](https://github.com/Onheiron/BillPouch/commit/03642d4bfc8d0113f457157a029c815ee46e2f1f))
* **test:** serializza test CEK con ENV_LOCK per evitare race su XDG_DATA_HOME ([32ec1b5](https://github.com/Onheiron/BillPouch/commit/32ec1b55c2dd601ad9eeacb113082039d664eab5))
* **test:** unwrap_err on Result&lt;Identity&gt; requires Debug, use .map(|_| ()) ([f4c1f33](https://github.com/Onheiron/BillPouch/commit/f4c1f33ea2f5f24ef6fb4bff86f8fe21dc6eea6e))
* **test:** usa path-based helpers nei test CEK, elimina dipendenza da XDG_DATA_HOME/HOME ([0fa1c46](https://github.com/Onheiron/BillPouch/commit/0fa1c4608eed4097aaf5328f896cbfe8aca48959))

## [0.1.4](https://github.com/Onheiron/BillPouch/compare/billpouch-v0.1.3...billpouch-v0.1.4) (2026-03-24)


### Features

* **api:** aggiungi route pause/resume/farewell-evict/invite a bp-api ([9596bfd](https://github.com/Onheiron/BillPouch/commit/9596bfd2063f3be8859537e206720a0e1527b026))
* **api:** web dashboard — embedded SPA served at GET / ([d4cfa3b](https://github.com/Onheiron/BillPouch/commit/d4cfa3b0ee610d4b01189fd3e01272071759a683))
* bp flock mostra tier per servizi Pouch, fix hint join obsoleta ([59fae24](https://github.com/Onheiron/BillPouch/commit/59fae2429612312e0fdcf9a27d2b527d28a00c3e))
* bp-api — REST API gateway Axum (9 endpoint, proxy su Unix socket) ([6e5ca17](https://github.com/Onheiron/BillPouch/commit/6e5ca17ed9d8d4ac275a22b737ccad741ef26550))
* CEK hints persistence (cek_hints.json) — files survive daemon restart ([3fbf8fd](https://github.com/Onheiron/BillPouch/commit/3fbf8fd4242e767cb126b78044a5535ff799c720))
* **cli:** aggiunge comando bp status — identita daemon e servizi in formato compatto ([48242c7](https://github.com/Onheiron/BillPouch/commit/48242c7f076fc1e005c71fde9d906ded00a438a0))
* **cli:** bp bootstrap list/add/remove — gestione WAN bootstrap nodes ([d709864](https://github.com/Onheiron/BillPouch/commit/d709864001ee49686f886d9bf39ba921461215c5))
* **cli:** bp farewell --evict + StorageManager::purge() + ControlRequest::FarewellEvict ([906746e](https://github.com/Onheiron/BillPouch/commit/906746ef020e462a3cdc8b861704c3100d8cc8cd))
* **cli:** bp hatch pouch --tier T1-T5, one-Pouch-per-network enforcement ([9ff1b32](https://github.com/Onheiron/BillPouch/commit/9ff1b32cced18653028c5fa40d02c852361f9f98))
* **cli:** bp leave --force auto-evict — protocol, server, CLI, wiki aggiornati ([3ab7de3](https://github.com/Onheiron/BillPouch/commit/3ab7de3138703197ebf5a6c174ab07ba8a7931ac))
* **cli:** bp leave with service precondition check + Leave handler upgrade ([fad87a6](https://github.com/Onheiron/BillPouch/commit/fad87a6e466da261213fd80b1526653ae6f45cc5))
* **cli:** bp pause --eta / bp resume + ServiceStatus::Paused + ControlRequest::{Pause,Resume} ([b229c7e](https://github.com/Onheiron/BillPouch/commit/b229c7eec3529bd538412737b0c81bdd2b18a2ce))
* **core:** adaptive k/n in PutFile via QoS + update protocol and CLI ([616323e](https://github.com/Onheiron/BillPouch/commit/616323ea3112cd9effa7312c09e5c6eaee9d076b))
* **core:** add QoS tracking, adaptive k params, and file manifest ([1e49a7f](https://github.com/Onheiron/BillPouch/commit/1e49a7f74cc053463adda57975fdd21ce950c3f2))
* **core:** add RLNC erasure coding (GF(2^8), encode/recode/decode) and StorageManager ([77dbf6a](https://github.com/Onheiron/BillPouch/commit/77dbf6aaf9472cf851e5bee9e876b0e3244f6ac3))
* **core:** Bootstrap nodes — scoperta WAN via bootstrap.json ([16ff6ed](https://github.com/Onheiron/BillPouch/commit/16ff6edc87d3e6a4a1b6e982f18728154b84383d))
* **core:** FragmentIndex gossip — RemoteFragmentIndex, AnnounceIndex, targeted GetFile fetch ([5bf7c86](https://github.com/Onheiron/BillPouch/commit/5bf7c8620a6ec04e52526ec8189131a0615038fe))
* **core:** network quality monitor — Ping/Pong RTT loop ([dc06e5e](https://github.com/Onheiron/BillPouch/commit/dc06e5ec8f8e597ee74f124aa63d0ab47fe8ca78))
* **core:** Persistenza Kademlia — KadPeers save/load su disco ([9ffa920](https://github.com/Onheiron/BillPouch/commit/9ffa9208d3936783d7dd7fa5b6eb47936974bb82))
* **core:** Proof-of-Storage challenge — fault score, OutgoingAssignments, PoS loop ([7461367](https://github.com/Onheiron/BillPouch/commit/746136780c9b437040892ffcc2e9f417239cb327))
* **core:** Rigenerazione Preventiva — rerouting frammenti quando fault_score &gt;= FAULT_SUSPECTED ([599aade](https://github.com/Onheiron/BillPouch/commit/599aade5378566208977f22d5f5e2079083fefa4))
* **identity:** multi-device export/import — bp export-identity / bp import-identity ([487a26d](https://github.com/Onheiron/BillPouch/commit/487a26de9163a38ebe376220a931f8ca352205e8))
* **marketplace:** storage marketplace — offers, acceptances, agreements ([b9fd5a7](https://github.com/Onheiron/BillPouch/commit/b9fd5a79aee25679b7d8675b1bd1166ad6754174))
* **nat:** AutoNAT + relay client for NAT traversal ([90cec5e](https://github.com/Onheiron/BillPouch/commit/90cec5e179ca6b0363dc5abb3ea8d273f4c725ea))
* **network:** ReputationTier R0-R4 + ReputationStore + DaemonState integration ([951f5ad](https://github.com/Onheiron/BillPouch/commit/951f5adaeeba3512c21fc2cf0677263cc9240b42))
* P2P fragment distribution and remote collection ([b8d1fb8](https://github.com/Onheiron/BillPouch/commit/b8d1fb813c546609cf1934ef369695dcda4da456))
* **playground:** add interactive local P2P network simulator ([40db85a](https://github.com/Onheiron/BillPouch/commit/40db85ac9d57aa25a60f06044bd6f58f6065bfed))
* PutFile/GetFile, StorageManager integration, fragment exchange, bp put/get CLI ([116c3ab](https://github.com/Onheiron/BillPouch/commit/116c3aba1d79eef97774848b8bf8dcd006ea6969))
* **security:** CEK per-user encryption + NetworkMetaKey as random secret ([23780cb](https://github.com/Onheiron/BillPouch/commit/23780cb32f5ab78ff776a01b6ef714be41e69908))
* **security:** encryption at rest — ChaCha20-Poly1305 per chunk before RLNC ([b0ada4a](https://github.com/Onheiron/BillPouch/commit/b0ada4aef3e5c04b5f5797d84051ea084a538a10))
* **security:** invite system — signed+encrypted network invite tokens ([6be3981](https://github.com/Onheiron/BillPouch/commit/6be398192062296c0440d59367578a8387ac8951))
* **security:** passphrase-protected identity (Argon2id + ChaCha20-Poly1305) ([f4045c5](https://github.com/Onheiron/BillPouch/commit/f4045c53d97a001541bff034f9a652a8517ccd8f))
* **storage:** StorageTier T1-T5 + BpError::InvalidInput ([52830c8](https://github.com/Onheiron/BillPouch/commit/52830c8f1e53ec2c12e9f9be1c98bb8fdc456d08))


### Bug Fixes

* aggiunge force:false a tutti i literal ControlRequest::Leave nei test e in bp-api ([a5cf17b](https://github.com/Onheiron/BillPouch/commit/a5cf17bd9bf67f9e34520271385c3aa9f0ac9fae))
* **api:** corregge CreateInvite fields, rewrite redeem senza ControlRequest inesistente ([6113f53](https://github.com/Onheiron/BillPouch/commit/6113f530f49705e11e630a87fc91a88d3ab91b03))
* **ci:** coverage - mkdir output dir, remove || true, fail_ci_if_error true ([4023beb](https://github.com/Onheiron/BillPouch/commit/4023bebfdfc3563e9b18d24fadc3b476e9b22edf))
* **ci:** remove invalid --all-features flag from cargo deny check ([65b2999](https://github.com/Onheiron/BillPouch/commit/65b299997a5db669dc92e7ddb4a7befd93a0d640))
* **ci:** security jobs red+continue-on-error at job level, coverage back to architecture_test ([576e6ba](https://github.com/Onheiron/BillPouch/commit/576e6ba8a2f240ac411fff90e661b9e6eb2883b4))
* **ci:** security jobs show yellow warning on step, not green, with step summary ([e4d0b4f](https://github.com/Onheiron/BillPouch/commit/e4d0b4fe11ce34e13690bf416ce30bb1166959ee))
* **cli:** add clap env feature for BP_PASSPHRASE env var support ([214a1fd](https://github.com/Onheiron/BillPouch/commit/214a1fdeaa121bf25cbacf256a14fe99ac4e4c60))
* **clippy:** add .. to ReservationReqAccepted pattern (renewal+limit fields) ([2439865](https://github.com/Onheiron/BillPouch/commit/2439865bf6b7baaae297915a30591fea79fa0ce9))
* **clippy:** allow too_many_arguments on handle_swarm_event ([bd294f7](https://github.com/Onheiron/BillPouch/commit/bd294f7105f557abc7beeba9c859e003d03286ce))
* **clippy:** allow too_many_arguments on run_network_loop ([2d5b3ee](https://github.com/Onheiron/BillPouch/commit/2d5b3eea743a0ec83ebbc51a10b315a788a57c4e))
* **clippy:** clone_on_copy — PeerId is Copy, use *peer_id ([031b85b](https://github.com/Onheiron/BillPouch/commit/031b85bd17921c8f44af0fb9f7f495df3c1a216f))
* **clippy:** introduce StorageManagerMap type alias to reduce type complexity ([e039109](https://github.com/Onheiron/BillPouch/commit/e0391091a79a7839673621f3f8fbe01bc4a27faa))
* **clippy:** remove unnecessary libp2p:: qualification for Multiaddr in server.rs ([1ae4a44](https://github.com/Onheiron/BillPouch/commit/1ae4a4457a29db826e69523071b57006144044b8))
* **clippy:** remove unused PeerQos import in quality_monitor tests ([7d199a1](https://github.com/Onheiron/BillPouch/commit/7d199a1e22837d43439d9d95c333f6af1945f393))
* **clippy:** remove unused PutFileResponse struct and Serialize import ([678007e](https://github.com/Onheiron/BillPouch/commit/678007e78e9f8a983e4db76047607ffff616ebb7))
* **clippy:** replace needless range loops with iter_mut/zip in Gaussian elimination ([3f8ee9d](https://github.com/Onheiron/BillPouch/commit/3f8ee9df1bf293863050f079f079afa39bf73241))
* **cli:** remove redundant trailing semicolon in Login dispatch ([871bfc2](https://github.com/Onheiron/BillPouch/commit/871bfc207905aaf321ff663fa4d3732899218d39))
* **cli:** use client.request() instead of .call() in relay connect + explicit type annotation ([1b02520](https://github.com/Onheiron/BillPouch/commit/1b025205cf61ee72a6ac2e47ef38b56ee224d854))
* **core:** add use&lt;'a, '_&gt; bound to pointers_for_peer return type ([b3c7637](https://github.com/Onheiron/BillPouch/commit/b3c763773e6c0c946c883f3bd67670013bb89e62))
* **core:** borrow &all_fragments in rlnc::decode call ([7dcb05c](https://github.com/Onheiron/BillPouch/commit/7dcb05c80b2a43e752c676644ed8e016c844effd))
* **core:** clone metadata before move into ServiceInfo::new ([cd2d3c9](https://github.com/Onheiron/BillPouch/commit/cd2d3c9e5427dc46aad4978af995f2c5ce3a3dc4))
* **core:** fix clippy warnings in params.rs (unused var, excessive precision) ([6c691bc](https://github.com/Onheiron/BillPouch/commit/6c691bc2dc2a07dc9f0d32519b67fad79434be45))
* **core:** fix reversed Horner coefficients in erfc_approx ([a78636c](https://github.com/Onheiron/BillPouch/commit/a78636c7a6104569fdee435bebc7279c32b1a8e8))
* **core:** name lifetimes explicitly in pointers_for_peer to fix use&lt;&gt; capture ([bd9d27b](https://github.com/Onheiron/BillPouch/commit/bd9d27bf1f86a2c0a8a9d757e1d04042bbe4f261))
* **core:** use&lt;'a, '_, '_&gt; for two anonymous lifetimes in pointers_for_peer ([5c775fa](https://github.com/Onheiron/BillPouch/commit/5c775fa0c2ef99b5d856a1c886bc7bb3a3955ffa))
* **dashboard:** aggiorna index.html a v0.3 (local_services, status, rimuovi marketplace) ([5d14e64](https://github.com/Onheiron/BillPouch/commit/5d14e64dcb4346ce7a22c11da43c9c7fed1ff9f4))
* **docs:** escape &lt;PeerId&gt; HTML tag in bootstrap doc comment ([28bcfb1](https://github.com/Onheiron/BillPouch/commit/28bcfb1c3da08138ce809cc3a8905f85d7b8b5b8))
* **docs:** fix rustdoc private/broken intra-doc links in quality_monitor ([c418cdc](https://github.com/Onheiron/BillPouch/commit/c418cdc5dd64293d29eb3b9722659b2f57e82134))
* **docs:** remove broken intra-doc link to DaemonState in reputation.rs ([ac38591](https://github.com/Onheiron/BillPouch/commit/ac385910b144e55f14e2e3ef78f8ded07a2c9fa4))
* **docs:** remove private intra-doc links in qos.rs ([7a9dee4](https://github.com/Onheiron/BillPouch/commit/7a9dee46a466d65addf0a4cca96a87fb2f111f78))
* **docs:** replace broken ReceivedOffers link with AgreementStore ([f5b85cf](https://github.com/Onheiron/BillPouch/commit/f5b85cf775d15b75d97fa2c57396fea196539e32))
* **docs:** replace literal \\n with real newlines in flock.rs doc comment ([ccf1357](https://github.com/Onheiron/BillPouch/commit/ccf135733052f5269928004828e285ddb2f95dff))
* drop renewed field from ReservationReqAccepted pattern (not in libp2p-relay 0.18) ([1d23d51](https://github.com/Onheiron/BillPouch/commit/1d23d51fb8f697777056372d8b53f48b77a8e94d))
* escape curly braces in Mermaid flowchart node labels ([b88fb1a](https://github.com/Onheiron/BillPouch/commit/b88fb1a340f6d53e2e3afda9365e795644ea864e))
* fully remove marketplace handlers from bp-api (leftover from partial edit) ([b029c9d](https://github.com/Onheiron/BillPouch/commit/b029c9deb2e5a47f35f37e4f6b715b1ab4912fa8))
* **invite:** add explicit imports in test module ([782649a](https://github.com/Onheiron/BillPouch/commit/782649a63527c2fe698de610a7d9f4e22463f58c))
* **invite:** remove spurious & in client.send() calls ([a551618](https://github.com/Onheiron/BillPouch/commit/a55161893e3eb89e79935f3cc10c108db99bb09c))
* re-add hidden bp join command to keep gossipsub mesh alive in smoke test ([fb58767](https://github.com/Onheiron/BillPouch/commit/fb587673da6bbd2d1e17e71a905b167917bc8ed0))
* remove AnnounceOffer variant from NetworkCommand enum ([2d2eca4](https://github.com/Onheiron/BillPouch/commit/2d2eca45c9897b501474ec62f3d54b31fd329475))
* remove bp join from smoke/playground entrypoints (gossip joined at hatch) ([669405a](https://github.com/Onheiron/BillPouch/commit/669405aeb992006ab49828493f27af083468ca04))
* remove spurious & in join.rs send call ([3aebbfb](https://github.com/Onheiron/BillPouch/commit/3aebbfb5fa534d02685a30b42dc0f3d3738e3bc4))
* replace non-ASCII em dash in byte string literal in storage/mod.rs ([9c157be](https://github.com/Onheiron/BillPouch/commit/9c157bec00c19833d4cd36b2e81d4d6ea6b39aa6))
* **rlnc:** guard recode() against all-zero coding vectors (production correctness) ([747e1e7](https://github.com/Onheiron/BillPouch/commit/747e1e702d42c02d7ff4b542a07f60bdf50a17af))
* **rlnc:** systematic encoding + deterministic tests ([3fe12ff](https://github.com/Onheiron/BillPouch/commit/3fe12fff68a00d1297d73fd976f22ff9bba2012a))
* **server:** Leave handler — map(|s| (*s).clone()) per &&ServiceInfo ([b146ed9](https://github.com/Onheiron/BillPouch/commit/b146ed94d2976878a65920b753f1500e1d01e3c9))
* StatusData.local_services in integration_test (was .services) ([8ec75dc](https://github.com/Onheiron/BillPouch/commit/8ec75dc8a649becf23ef7468e1289190bdac87f4))
* **test+clippy:** add storage_managers to test DaemonState; use clamp() in put.rs ([95e8a89](https://github.com/Onheiron/BillPouch/commit/95e8a89d2cbb1ff767a87c4f499dc9c379b6d2c1))
* **test:** add agreements field to DaemonState in architecture_test ([78eaaa2](https://github.com/Onheiron/BillPouch/commit/78eaaa2836334b437437b949b724711dfe9e86e0))
* **test:** add storage_managers to DaemonState in integration_test.rs ([6a37a44](https://github.com/Onheiron/BillPouch/commit/6a37a44f899d7bc81b4ef3aa8fe05a0256409f3c))
* **test:** config_path test acquires IDENTITY_FS_LOCK to avoid HOME race condition ([854861f](https://github.com/Onheiron/BillPouch/commit/854861f02472074cb86877e54f3ffd6238acc0ff))
* **test:** correggi assert identity_export_con_passphrase -- controlla ciphertext invece di enum Encrypted ([afa7aaf](https://github.com/Onheiron/BillPouch/commit/afa7aafd0c3059e08a5f67ff624df583a514f2ab))
* **test:** persist_cek_hints crea dir, mutex poisoning con unwrap_or_else ([06a6658](https://github.com/Onheiron/BillPouch/commit/06a665814a6b7e718a9cb70f1d8e11d6db40e8f9))
* **test:** recode from full k-rank set to avoid singular matrix in decode_from_recoded_fragments ([343d1b8](https://github.com/Onheiron/BillPouch/commit/343d1b8b7728956a974457c3a1c82349e5aa96f0))
* **tests:** add missing qos field to DaemonState in test helpers ([03642d4](https://github.com/Onheiron/BillPouch/commit/03642d4bfc8d0113f457157a029c815ee46e2f1f))
* **test:** serializza test CEK con ENV_LOCK per evitare race su XDG_DATA_HOME ([32ec1b5](https://github.com/Onheiron/BillPouch/commit/32ec1b55c2dd601ad9eeacb113082039d664eab5))
* **test:** unwrap_err on Result&lt;Identity&gt; requires Debug, use .map(|_| ()) ([f4c1f33](https://github.com/Onheiron/BillPouch/commit/f4c1f33ea2f5f24ef6fb4bff86f8fe21dc6eea6e))
* **test:** usa path-based helpers nei test CEK, elimina dipendenza da XDG_DATA_HOME/HOME ([0fa1c46](https://github.com/Onheiron/BillPouch/commit/0fa1c4608eed4097aaf5328f896cbfe8aca48959))

## [Unreleased] — v0.3.0-dev

### Breaking Changes

- `bp hatch pouch --storage-bytes <N>` rimosso — sostituito da `--tier <T1..T5>`
- `bp join` rimosso dal CLI pubblico — resta hidden per uso interno script
- Storage marketplace rimosso: `bp offer`, `bp agree`, `bp offers`, `bp agreements`, REST `/marketplace/*`

### Added

- **`StorageTier` T1–T5** (`storage/tier.rs`) — tier fissi 10 GiB / 100 GiB / 500 GiB / 1 TiB / 5 TiB con `quota_bytes()`, `participating_tiers()`, `parse()`, serde
- **`ReputationTier` R0–R4** (`network/reputation.rs`) — tier discreti di reputazione basati su storico uptime e PoS, `ReputationRecord`, `ReputationStore` in `DaemonState`
- **`ServiceStatus::Paused`** (`service.rs`) — con `eta_minutes` e `paused_at`; aggiornati `Stopping` e `Error(String)`
- **`ControlRequest::Pause` / `Resume`** — manutenzione temporanea con announcement gossip
- **`ControlRequest::FarewellEvict`** — eviction permanente Pouch: purge storage + gossip `evicting=true` + penalità reputazione
- **`ControlRequest::Leave`** aggiornato — precondition check servizi attivi; risposta con `blocked: true` e lista hint di stop
- **`StorageManager::purge()` / `storage_summary()`** — rimozione definitiva storage su disco
- **One-Pouch-per-network enforcement** — il daemon rifiuta un secondo `hatch pouch --network X` sulla stessa identità
- **`bp pause <service_id> --eta <minutes>`** / **`bp resume <service_id>`** — nuovi comandi CLI
- **`bp farewell --evict`** — flag eviction permanente
- **`BpError::InvalidInput`** — nuova variante per input non validi (es. tier sconosciuto)
- **CEK hints persistence** (`cek_hints.json`) — `load_cek_hints()` + `persist_cek_hints()` in `server.rs`; le chiavi di cifratura sopravvivono al riavvio del daemon
- **`bp flock` tier display** — i servizi Pouch mostrano il tier (`tier: T2`) nella lista locale
- **`bp status`** — nuovo comando compatto: identità daemon (fingerprint, alias, peer_id, versione), servizi attivi, reti, conteggio peer
- **`bp leave --force`** — auto-evict di tutti i servizi attivi sul network (Pouch: eviction permanente; Bill/Post: stop graceful), poi leave; `force: bool` aggiunto a `ControlRequest::Leave` con `#[serde(default)]` per retrocompatibilità
- **`ControlRequest::Status`** + **`StatusData`** — payload strutturato con `peer_id`, `fingerprint`, `alias`, `local_services`, `networks`, `known_peers`, `version`
- **`LeaveData`** struct — payload tipizzato per risposte Leave
- **bp-api route v0.3** — `POST /services/:id/pause`, `POST /services/:id/resume`, `DELETE /services/:id?evict=true`, `POST /invites`, `POST /invites/redeem`

### Tests

- Nuovi test architettura (sezioni 11–13): `StorageTier`, `ReputationTier`, `ServiceStatus::Paused` lifecycle
- Nuovi test architettura: `ControlRequest::Status` serialization roundtrip; `StatusData` field completeness
- Nuovi integration test: `pause_resume_roundtrip`, `farewell_evict_removes_service`, `leave_blocked_by_active_service`, `hatch_second_pouch_same_network_rejected`

## [0.1.3](https://github.com/Onheiron/BillPouch/compare/billpouch-v0.1.2...billpouch-v0.1.3) (2026-03-18)


### Bug Fixes

* **release:** build binaries in release-please workflow ([9ffe8e2](https://github.com/Onheiron/BillPouch/commit/9ffe8e22111c30349356b4d26e0b9bf3189da75d))

## [0.1.2](https://github.com/Onheiron/BillPouch/compare/billpouch-v0.1.1...billpouch-v0.1.2) (2026-03-18)


### Bug Fixes

* **release:** trigger on billpouch-v* tags from release-please ([7ad0516](https://github.com/Onheiron/BillPouch/commit/7ad051678f3b8f12d2ccf39203f6d53fe71411a6))

## [0.1.1](https://github.com/Onheiron/BillPouch/compare/billpouch-v0.1.0...billpouch-v0.1.1) (2026-03-18)


### Features

* **smoke:** add Docker Compose smoke test with 3 P2P nodes ([2ea2ab8](https://github.com/Onheiron/BillPouch/commit/2ea2ab85c57d72afb96a7b26948257c247cbc79d))


### Bug Fixes

* **ci:** accept any swarm build failure on CI environments ([71f2569](https://github.com/Onheiron/BillPouch/commit/71f2569fa81ff4f2698fa641909463d030758ae1))
* **ci:** fix failing GitHub Actions workflows ([1d84dba](https://github.com/Onheiron/BillPouch/commit/1d84dbafbd7a2bb1cf3742415e912cb018a5681a))
* **ci:** fix failing GitHub Actions workflows ([332b854](https://github.com/Onheiron/BillPouch/commit/332b854683c1510b1e061f9996fd621566538313))
* **ci:** fix formatting, release-please, and coverage workflows ([430bb64](https://github.com/Onheiron/BillPouch/commit/430bb640e404e08c4bacf5b96663e72b75b4aa3b))
* **ci:** propagate mDNS errors instead of panicking in build_swarm ([d5d89bb](https://github.com/Onheiron/BillPouch/commit/d5d89bb3c5c086da04eb63e1abb0221dc37da570))
* **ci:** use #[tokio::test] for swarm test to provide Tokio runtime on Linux ([c2cc481](https://github.com/Onheiron/BillPouch/commit/c2cc481499a19ef2cb468fc292ece2d3615522be))
* **docker:** bump Rust image to 1.85 for edition2024 support ([d752b52](https://github.com/Onheiron/BillPouch/commit/d752b52ae0bd4b133d3444e8d644057e141f9c50))
* **smoke:** hatch services after mesh formation for gossipsub propagation ([576740c](https://github.com/Onheiron/BillPouch/commit/576740c126957fe7d793e79ec4733bb2b30bd05c))
* **smoke:** join network before mesh wait to keep connections alive ([d068e17](https://github.com/Onheiron/BillPouch/commit/d068e171b4f4ba9513fb30c24144b995ccec0760))
* **smoke:** match service type without brackets in Known Peers section ([38fc272](https://github.com/Onheiron/BillPouch/commit/38fc272ad5b66fd5d1a42a7faafdc666176d10cc))
* **smoke:** move network join into entrypoint for immediate gossipsub subscription ([1d51b83](https://github.com/Onheiron/BillPouch/commit/1d51b83de33239044d3230653aa69e8220e1bd37))
* **smoke:** use safe arithmetic to avoid set -e exit on zero increment ([3045fbd](https://github.com/Onheiron/BillPouch/commit/3045fbde340f55ae2f702e0284a9f3ccbf312db2))

## [0.1.0] — 2026-03-18

### Features

- Ed25519 identity management (login/logout/fingerprint)
- Three service types: pouch (storage), bill (file I/O), post (relay)
- Service registry with UUID-based instance tracking
- libp2p swarm with gossipsub, Kademlia, mDNS, Noise, Yamux
- Gossip-based NodeInfo DHT with stale peer eviction
- Unix socket control protocol (CLI <-> daemon, newline-delimited JSON)
- Multi-network support (join/leave independent networks)
- CLI commands: login, logout, hatch, flock, farewell, join
- Extensible metadata on NodeInfo for future protocol extensions

### Testing

- 43 architecture verification unit tests
- Integration tests for control protocol over Unix socket

### CI/CD

- GitHub Actions: CI (fmt, clippy, test), security audit, coverage, docs
- Cross-platform release builds (Linux x86_64, macOS x86_64/aarch64)
- GitHub Pages documentation site

[0.1.0]: https://github.com/Onheiron/BillPouch/releases/tag/v0.1.0
