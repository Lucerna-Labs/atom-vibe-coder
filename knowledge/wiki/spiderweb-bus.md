# Spiderweb Bus Runtime
tags: spiderweb, bus, fabric, flow, route

Every proof run must emit L0 transport, L1 message, L2 flow, and L3 orchestration envelopes. Ramps and off-ramps make unsupported provider or evidence paths visible blockers instead of silent fallbacks.

The layer invariant is route-specific. A proof decision passes only when the active parent chain touches L0 transport, L1 message, L2 flow, and L3 orchestration; old traffic elsewhere on the bus cannot satisfy the current proof.

Provider execution is a continuing thread, not a side channel. A prepared provider call must be lifted through a parented L0 on-ramp from the pending proof route, scheduled through L1/L2/L3, and then return success or failure through the same route family before proof capture or blocking is allowed.

[[bus:spiderweb]]
[[spiderweb-proof-loop]]
