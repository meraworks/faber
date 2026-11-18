# Fusion thread evidence

REA with Ghidra 12.1.4 inspected the installed `FusionXInterface10.dll`.
SHA-256: `1e5ab1786e59abb2c2f607e9111e584b75c74448b7b618c8673a9caefe2e71cd`.
The evidence bundle is `.cargo/fusion-thread-evidence.json`.

`ThreadInfo::create_raw` at `0x1800106e0` builds named arguments for thread
type, designation, class and handedness, then calls a class factory. It also
passes the taper and internal-thread flags.
Evidence: `ev_53094f26f26367737e53182313b023dc04600eaacda009085008e8d330e5b2a5`.

`ThreadDataQuery::create_raw` at `0x180010420` passes the taper flag to the
class factory. This separates thread data queries from geometry construction.
Evidence: `ev_52cdb127aca21098fbf61c874aa0865e1fc4c241fd19b2d02db2cf0a0901cb57`.

The installed `CPP/include/Fusion/Features/ThreadFeatureInput.h` declares
modelled threads, full length, partial length and offset. It declares cylindrical
input faces and directs handedness control to `ThreadInfo`. These are interface
declarations, not observations of a running modelling command.

## Decision and limits

Keep table lookup separate from modelled geometry. A display annotation does
not supply a printable ridge. Faber uses its existing helix and sweep operations
for a geometry demonstration. Do not claim that the factory wrapper reveals
Fusion's thread construction algorithm or proves a standard tolerance class.
The native implementation behind the factory remains unverified.
