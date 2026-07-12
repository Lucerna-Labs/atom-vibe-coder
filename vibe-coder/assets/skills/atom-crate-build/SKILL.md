---
name: atom-crate-build
description: Step 3. Build each frozen crate completely and independently in topological order.
---

# Atom Crate Build

Build one crate at a time in the frozen topological order. Each crate must own
one complete responsibility, honor its message contracts, compile with warnings
denied, and pass real unit tests before the next crate starts.

Reject `todo!`, `unimplemented!`, placeholder or stub behavior, broad warning
allows, hidden direct subsystem calls, and half-implemented seams. The sole
temporary warning exception is a scoped `dead_code` or `unused` allow carrying
an exact `COUPLE: <consumer>` marker. Every marker is tracked in the ledger and
must be removed during coupling.

Do not connect crates in this step. All crates pass before coupling begins.
