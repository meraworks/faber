# Windows kernel research

Date: 2026-11-17

## Decision

Use OCCT 7.9.3 shared libraries with a small C ABI in `faber-kernel-occt`.
Keep opaque solid handles behind the neutral `Kernel` port. Keep the B-rep
shape as the model. Generate meshes only for display and mesh export.
This is the shortest setup route found for this machine. It does not require
an OCCT source build. This is a proposed choice, not a completed build check.

OCCT 7.9.3 is a maintenance release with fixes to booleans, fillets,
chamfers, pipe shells and STEP input. OCCT 8 changes core APIs, including
exception types. Pin 7.9.3 for the first adapter and assess 8 separately.
[Release notes](https://github.com/Open-Cascade-SAS/OCCT/releases/tag/V7_9_3)
[OCCT 8 notes](https://github.com/Open-Cascade-SAS/OCCT/releases/tag/V8_0_0)

## Downloads

The official release API reports these files:

| File | Bytes | Use |
| --- | ---: | --- |
| `opencascade-7.9.3-vc14-64.zip` | 55,147,765 | Release SDK only |
| `3rdparty-vc14-64.zip` | 193,789,890 | Third-party packages |
| `opencascade-7.9.3-vc14-64-combined.zip` | 258,814,881 | SDK and third-party packages |

Use the [combined archive](https://github.com/Open-Cascade-SAS/OCCT/releases/download/V7_9_3/opencascade-7.9.3-vc14-64-combined.zip).
Its SHA-256 is
`afbef3457fbc4a2bdca0608e0fe284392f51e4c2c42ccbfb9df7168d8e4eb9b3`.
The [SDK archive](https://github.com/Open-Cascade-SAS/OCCT/releases/download/V7_9_3/opencascade-7.9.3-vc14-64.zip)
has SHA-256
`2a4f5e187992427674869e89477882d1ace4d67695d4b27173830d93f8cf11be`.
[Release asset data](https://api.github.com/repos/Open-Cascade-SAS/OCCT/releases/tags/V7_9_3)

The SDK archive was read in memory. It contains these files below
`opencascade-7.9.3-vc14-64/`:

- `inc/TopoDS_Shape.hxx` and `inc/Standard_Version.hxx`
- `win64/vc14/lib/TKernel.lib`
- `win64/vc14/bin/TKernel.dll` and `TKDESTEP.dll`
- `cmake/OpenCASCADEConfig.cmake`
- `env.bat`

This is a development SDK with headers, import libraries and runtime DLLs.
The combined archive was not downloaded. The official packaging workflow
puts the same SDK and `3rdparty-vc14-64` in that archive.
It enables TBB and jemalloc, plus several display and exchange dependencies.
Do not assume that the SDK alone can run. Inspect each selected DLL with
`dumpbin /DEPENDENTS` and include its runtime dependencies. The full third-party
archive is the simplest first setup; reduce the shipped set after inspection.
[Package workflow](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_9_3/.github/workflows/build-windows-packages.yml)

## Local tools

Read-only checks found:

- Visual Studio Build Tools 2022, version 17.14.37411.7
- MSVC tools 14.44.35207
- CMake 3.31.6-msvc6 in the Visual Studio installation
- Rust 1.99.0, host `x86_64-pc-windows-msvc`

`cl` and `cmake` are absent from the current `PATH`. No tool installation is
needed for the proposed first build. Run these in the existing PowerShell
process; do not open a new window:

```powershell
$vs = 'C:/Program Files (x86)/Microsoft Visual Studio/2022/BuildTools'
Import-Module "$vs/Common7/Tools/Microsoft.VisualStudio.DevShell.dll"
Enter-VsDevShell -VsInstallPath $vs -SkipAutomaticLocation `
    -DevCmdArguments '-arch=x64 -host_arch=x64'
$env:PATH = "$vs/Common7/IDE/CommonExtensions/Microsoft/CMake/CMake/bin;$env:PATH"
$env:CARGO_BUILD_JOBS = '6'
$env:CMAKE_BUILD_PARALLEL_LEVEL = '6'
```

For a CMake bridge, use `-G "Visual Studio 17 2022" -A x64` and build with
`--config Release --parallel 6`. For a `cc` build script, use C++17, `/EHsc`
and `/MD`, with the SDK `inc` and `win64/vc14/lib` paths. Add the SDK runtime
and required third-party runtime paths for tests. Build the bridge with the
release CRT even when Rust uses its debug profile. Do not mix C++ debug STL
objects with release OCCT objects.

The package workflow uses VS 2022 and C++17. Microsoft supports binary
compatibility across these MSVC versions, subject to the linker and runtime
version rules. Confirm a small link and load test before the full adapter.
[MSVC compatibility](https://learn.microsoft.com/en-us/cpp/porting/binary-compat-2015-2017?view=msvc-170)

## Rust binding comparison

`opencascade-rs` supplies useful examples and many operations. Its default
`builtin` feature compiles OCCT from source. Turning this feature off permits
a preinstalled shared SDK. Its build script accepts OCCT major 7, minor 8 or
later; it rejects major 8. The bundled build disables many third-party
libraries and builds static OCCT. It still needs CMake and a C++ compiler.
[README](https://github.com/bschwind/opencascade-rs/blob/main/README.md)
[SDK detection](https://github.com/bschwind/opencascade-rs/blob/main/crates/opencascade-sys/build.rs)
[Bundled build](https://github.com/bschwind/opencascade-rs/blob/main/crates/occt-sys/src/lib.rs)

Do not adopt its current bindings without an exception audit. The inspected
boolean constructors and `Build` methods return ordinary CXX values, not
`Result`. Their helper constructor calls `new T(...)` without a catch block.
CXX calls `std::terminate` if such a call throws. These calls cannot reliably
turn a bad operation into a Faber error. This finding applies to the inspected
boolean path; it is not a claim that every binding lacks error handling.
[Boolean bridge](https://github.com/bschwind/opencascade-rs/blob/main/crates/opencascade-sys/src/b_rep_algo_api.rs)
[Constructor helper](https://github.com/bschwind/opencascade-rs/blob/main/crates/opencascade-sys/include/bindings_common.hxx)
[CXX exception rules](https://cxx.rs/binding/result.html)

The Faber C ABI should catch `Standard_Failure`, `std::exception` and other
C++ exceptions at each entry point. Return a status and owned error text.
Check operation status and shape validity before publishing a solid handle.
Keep allocation and deletion on the C++ side. Catching exceptions does not
recover access violations or kernel hangs. Test invalid inputs and failed
operations, as well as valid models. Start with serial kernel calls.

## Licence and checks

OCCT uses LGPL 2.1 with the OCCT exception. Keep the licence, exception and
required notices with distribution. The third-party packages have separate
terms. Dynamic linking provides a clear library boundary; it does not remove
licence duties. Check the selected runtime files before distribution.
[OCCT licence](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_9_3/dox/license.md)
[OCCT exception](https://github.com/Open-Cascade-SAS/OCCT/blob/V7_9_3/OCCT_LGPL_EXCEPTION.txt)

No software was installed or built during this research. The next check is a
small bridge that creates a solid, cuts a hole, computes volume, meshes it,
and writes and reads STEP. Then test fillet, revolve, loft and sweep. Compare
volume and bounds after STEP round trips. Keep these tests offline. Use at
most six build jobs and run only one Cargo build at a time.
