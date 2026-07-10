# WiFi Adapter Build Recipe
tags: wifi, wireless, adapter, driver, networking, hardware, security, dependency-free, production, recipe

Status: reference recipe. This recipe requires a named operating system, driver model, chipset or platform API, privileges, and real hardware matrix before implementation. Never invent radio, cryptographic, or kernel support. Related graph nodes: [[bus:spiderweb]], [[scan]], [[flow]], [[preserve]], [[measure]], [[compare]], [[order]], [[hash]].

## Step 01 Freeze the target and authority boundary
Name the OS, architecture, driver framework, minimum version, adapter or supported platform API, package/signing policy, and whether the deliverable is kernel driver, user-mode service, or controller UI. Gate: unsupported OS, chipset, and privilege combinations are explicit blockers.

## Step 02 Inventory hardware and platform capabilities
Discover adapters through documented OS interfaces, capture stable device identity, driver version, radio capabilities, supported bands, authentication modes, and current operational state. Never infer capability from a marketing name. Gate: discovery is tested with no adapter, one adapter, multiple adapters, disabled hardware, and surprise removal.

## Step 03 Define typed states and commands
Model absent, disabled, idle, scanning, authenticating, associating, obtaining-address, connected, roaming, disconnecting, and failed states. Commands have IDs, deadlines, cancellation, and legal source states. Gate: a complete transition table rejects impossible and stale commands.

## Step 04 Build the platform boundary
Wrap each native call, handle, buffer, callback, and status code behind a narrow typed interface. Validate lengths and lifetimes before crossing FFI. Keep secrets out of logs and durable proof. Gate: mock boundary tests cover every status and real boundary tests run on target hardware.

## Step 05 Implement bounded scanning
Issue scan requests, correlate completion, normalize network identity, channel, band, signal, security, and freshness, and deduplicate results without merging distinct BSSIDs. Gate: empty, crowded, hidden, duplicate-name, changing-signal, timeout, cancellation, and adapter-removal scans are exercised in real RF conditions.

## Step 06 Preserve credential and security authority
Use the operating system credential and authentication facilities; do not implement WPA, key derivation, certificate validation, or secret storage ad hoc. Accept secret references rather than durable plaintext. Gate: logs, bus envelopes, learning records, crash paths, and UI never contain credentials.

## Step 07 Implement connect orchestration
Validate requested network against capabilities, submit profile or credential reference, track authentication and association separately, enforce deadlines, and map native failures to stable typed errors. Gate: valid, wrong-secret, unsupported-security, weak-signal, cancellation, and removal cases leave one truthful state.

## Step 08 Verify address and route readiness
Association is not connectivity. Observe address assignment, DNS, default route, captive-portal indication, and declared reachability probes without masking partial failure. Gate: static, DHCP, IPv4-only, IPv6-only, no-DNS, no-route, and captive-network cases are distinguished.

## Step 09 Own disconnect, roam, suspend, and recovery
Make disconnect idempotent, invalidate stale callbacks, handle roaming and link loss, and preserve legal state across sleep/resume and adapter reset. Gate: repeated connect/disconnect, sleep during each phase, AP loss, and driver restart recover without stale success.

## Step 10 Route operations through the Spiderweb Bus
Use L0 for OS/device events, L1 for typed adapter commands and observations, L2 for scan/connect/connectivity flows, and L3 for lifecycle, cancellation, recovery, and proof. Explicit ramps separate credentials, platform calls, state, and UI. Gate: successful and failed real connections both produce complete auditable routes.

## Step 11 Bound concurrency and resource use
Serialize state transitions per adapter while allowing bounded observation work. Use generation IDs for callbacks, bounded scan lists, cancellation tokens, and explicit handle cleanup. Gate: stress, cancellation races, rapid radio toggles, and surprise removal show no deadlock, leak, duplicate completion, or stale state.

## Step 12 Package installation and rollback
Produce the correct signed package or user-mode deployment, least-privilege configuration, versioned migration, diagnostics, uninstall, and rollback. Never claim kernel-driver readiness without platform signing and installation evidence. Gate: clean install, upgrade, failed upgrade, rollback, and uninstall run on disposable target systems.

## Step 13 Run the real hardware matrix
Test every supported OS/architecture and representative adapters/APs across bands, security modes, signal levels, congestion, sleep/resume, roaming, repeated cycles, and long sessions. Measure connection correctness and recovery separately from speed. Record hardware IDs, driver versions, commands, logs with secrets redacted, and artifact hashes. Simulation or a smoke scan cannot promote the recipe.

