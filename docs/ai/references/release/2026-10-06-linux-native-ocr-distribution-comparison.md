# Linux Native OCR Distribution Comparison

Date: 2026-10-06

Status: accepted distribution decision and supporting research.

## Question

How do established projects distribute Linux software that depends on
Tesseract, Leptonica, or a comparable native library, and which model best fits
AUV's CLI, long-running daemon, and npm/NAPI package?

## Observed distribution models

| Model | Representative evidence | What the project promises |
|---|---|---|
| External executable | `pytesseract`, OCRmyPDF, NormCap's Python package | The application is portable at its language-package layer; the OS or user supplies a compatible `tesseract` command and language data. |
| Distribution-native dependency | Debian's OCRmyPDF and gImageReader packages | The distro package manager installs an ABI-compatible Tesseract/Leptonica set and rebuilds consumers when the distro ABI changes. |
| Bundled application filesystem | Flatpak applications and AppImage | The application carries libraries and resources not guaranteed by the host, while retaining a deliberately small host ABI boundary. |
| Per-platform native package | `sharp` and its `@img/*` npm packages | Installation selects an OS/CPU/libc-specific addon and matching private native libraries; source build is exceptional. |
| WebAssembly runtime | Tesseract.js | Native Linux library ABI is removed, at the cost of a separate worker/runtime, model-data management, and different performance and feature constraints. |

### 1. External `tesseract` process

`pytesseract` explicitly requires an installed `tesseract` command, allows its
path to be configured, and invokes it as a subprocess. Missing Tesseract becomes
a feature-level `TesseractNotFoundError`, rather than a dynamic-loader failure
while importing the Python package. See its
[installation instructions](https://github.com/madmaze/pytesseract#installation)
and [`run_tesseract` implementation](https://github.com/madmaze/pytesseract/blob/master/pytesseract/pytesseract.py).

OCRmyPDF uses the same boundary at a larger production scale. Its official
installation guide says Tesseract is an external program that must be supplied
by the OS package manager, Homebrew, or its Docker image. Debian's package then
declares `tesseract-ocr (>= 4.0.0)` as a dependency. OCRmyPDF also searches
`PATH`, supports an alternate `TESSDATA_PREFIX`, checks versions, and can
disable OCR through an engine choice. See the
[upstream installation guide](https://github.com/ocrmypdf/OCRmyPDF/blob/main/docs/installation.md),
[advanced executable override documentation](https://github.com/ocrmypdf/OCRmyPDF/blob/main/docs/advanced.md),
and [Debian package metadata](https://packages.debian.org/trixie/ocrmypdf).

NormCap uses an external Tesseract executable too. Its source installation
instructions ask Linux users to install `tesseract-ocr`, its development logs
identify `/app/bin/tesseract` or `/usr/bin/tesseract`, and its Flatpak packages
that executable inside the sandbox. See the
[NormCap README](https://github.com/dynobo/normcap#use-python-package).

This model fits a generic CLI tarball when OCR is optional and missing OCR does
not prevent `auv --help`, `auv serve`, capture, input, or pairing from starting.
It also gives a daemon a useful process-isolation boundary and permits timeout
or cancellation. Its cost is that a raw GitHub tarball cannot claim that OCR is
ready until `tesseract` and the requested `.traineddata` files are present.

### 2. Distribution packages declare the native ABI

Direct in-process linkage is common when the application itself is delivered by
the distribution. Debian's gImageReader package depends on the distro's exact
`libtesseract5`; Debian's `libtesseract5` in turn depends on
`libleptonica6`. The package manager owns the ABI-compatible graph. See
[gImageReader package metadata](https://packages.debian.org/trixie/graphics/gimagereader-qt)
and [`libtesseract5` metadata](https://packages.debian.org/trixie/arm64/libtesseract5).

This is not evidence that one binary built on Ubuntu 24.04 is portable to every
glibc distribution. It is evidence for producing actual `.deb`/`.rpm` packages
whose dependencies name distro packages, or allowing distributions to rebuild
AUV. Under this model, source builds happen in package infrastructure, not on an
end user's machine.

### 3. Flatpak and AppImage bundle application dependencies

NormCap recommends Flatpak for Linux and its Flathub manifest builds Leptonica
1.85, Tesseract 5.5.1, and English trained data into `/app`; it sets
`TESSDATA_PREFIX=/app/share`. This creates a controlled OCR ABI independent of
the host distribution. See the
[NormCap Flatpak manifest](https://github.com/flathub/com.github.dynobo.normcap/blob/master/com.github.dynobo.normcap.yml).

AppImage's official rule is to bundle dependencies that cannot reasonably be
assumed to exist on every target distribution, but to exclude low-level or
host-coupled libraries whose bundling can break integration. Its tooling can
copy ELF dependencies and arrange application-relative lookup. See the
[AppImage dependency model](https://docs.appimage.org/introduction/concepts.html)
and [native binary packaging guide](https://docs.appimage.org/packaging-guide/from-source/native-binaries.html).

The operational caveat is material for AUV: NormCap now marks its AppImage as
deprecated and says that, despite substantial effort, it was too unreliable;
Flatpak is its recommended Linux package. See the
[NormCap README](https://github.com/dynobo/normcap#installation) and
[changelog](https://github.com/dynobo/normcap/blob/main/CHANGELOG).

Flatpak is a strong optional desktop distribution for OCR and Portal-driven UI,
but it is not an automatic replacement for AUV's host user service. Sandbox
permissions, Unix-socket visibility, systemd user integration, and access to
host applications become explicit product contracts. AppImage can be a
supplemental portable package, but should not be treated as proof that a daemon
will integrate with every host desktop.

### 4. Native Node addons ship platform-specific dependency payloads

`sharp` is the closest mature analogue to AUV's npm/NAPI distribution. It
publishes ready-compiled addon and libvips packages for explicit OS, CPU, and
libc combinations. Linux glibc and musl are separate, minimum glibc versions
are documented, and the package manager selects the matching optional package
at install time. See the
[`sharp` installation matrix](https://sharp.pixelplumbing.com/install/#prebuilt-binaries),
[`sharp` optional dependencies](https://github.com/lovell/sharp/blob/main/package.json),
and [libvips packaging repository](https://github.com/lovell/sharp-libvips).

Node-API stabilizes the interface between Node.js and an addon, but Node's own
documentation explicitly warns that external libraries used by an addon need
their own ABI guarantee. Prebuilding a `.node` file therefore does not solve a
missing `libtesseract` or `liblept` dependency by itself. See
[Node-API's ABI implications](https://nodejs.org/api/n-api.html#implications-of-abi-stability)
and [precompiled binary guidance](https://nodejs.org/api/n-api.html#uploading-precompiled-binaries).

AUV already uses per-platform optional npm packages. Its current Linux NAPI
addon does not link the Linux driver; the same platform package also carries an
`auv` CLI sidecar, and that sidecar is what inherits the Tesseract/Leptonica
dependency. The release contract must therefore validate both `require()` or
`import` of the `.node` file and execution of the packaged sidecar. If AUV
bundles OCR libraries for npm, they should be an OS/CPU/libc-specific private
payload beside the sidecar, with application-relative lookup and license files,
following the `sharp` model.

### 5. WebAssembly removes the Linux shared-library ABI

Tesseract.js compiles Tesseract to WebAssembly and runs it in a worker. It can
work in browsers and Node.js without host Tesseract/Leptonica shared objects,
but it still manages worker code, one of several WASM cores, and language
models. Its documentation recommends reusing a worker and caching trained data;
it notes that code and data can be a meaningful lazy-loaded payload. See the
[Tesseract.js README](https://github.com/naptha/tesseract.js/blob/master/README.md),
[worker API](https://github.com/naptha/tesseract.js/blob/master/docs/api.md),
and [performance guidance](https://github.com/naptha/tesseract.js/blob/master/docs/performance.md).

WASM is credible for a browser-facing recognition provider or an eventual
sandboxed fallback. Replacing AUV's current native OCR with it would be a
separate performance and behavior project, not a release-packaging fix.

## Implications for AUV

The established projects do **not** converge on asking end users to compile the
application. They choose a dependency owner appropriate to the channel:

- distro package: the distro owns the native dependency graph;
- language-level package or simple tarball: Tesseract is an external executable
  and missing OCR is reported at feature use;
- portable desktop package: the application filesystem owns Tesseract,
  Leptonica, and selected language data;
- npm native package: the publisher owns per-platform binaries and private
  native dependencies, with explicit OS/CPU/libc selection;
- browser/sandbox: WASM owns the OCR runtime and separately manages model data.

AUV's current generic Linux artifact has the least portable combination: a
single executable dynamically links an OCR ABI selected by the build runner,
while the archive contains neither the matching libraries nor distro package
metadata. Because the dependency is loaded before `main`, even non-OCR commands
fail on a host with a different SONAME.

## Accepted distribution contract

The owner selected a full default Linux bundle instead of changing the OCR
provider boundary in this slice. The contract is:

1. Keep the current in-process `leptess` provider and leave Windows and macOS
   behavior unchanged.
2. Build Linux release binaries and NAPI bindings on Debian 12, giving the
   glibc artifacts an explicit GLIBC 2.36 baseline. Debian 12 is the oldest
   practical baseline for the current dependency graph: it provides PipeWire
   0.3.65 headers, while Ubuntu 22.04 provides 0.3.48 and cannot compile
   `pipewire-rs` 0.8.
3. Make the existing Linux archive names self-contained. Each archive keeps
   `auv` at its root and includes a private ELF dependency closure under
   `lib/auv`, English trained data under `share/auv/tessdata`, dependency
   copyright files, and a checksummed manifest.
4. Keep glibc and the ELF loader as the host ABI boundary. A release check must
   reject any executable or bundled library that requires a newer glibc symbol
   than 2.36.
5. Preserve `TESSDATA_PREFIX` as an explicit override. Otherwise, AUV uses the
   bundled tessdata directory only when it contains every requested language
   model; this prevents the bundled English baseline from hiding additional
   system-provided models.
6. Stage the complete Linux archive directory in the OS/CPU/glibc-specific npm
   platform package. Copying only `bin/auv` would break its relative library and
   tessdata lookup.
7. Exercise the final archives on clean Debian 12, Debian 13, Ubuntu 24.04, and
   Ubuntu 26.04 containers with no Tesseract or Leptonica packages installed.
8. Ship the PipeWire client files that match the bundled `libpipewire`: the
   modules and SPA plugins named by Debian 12's `client.conf` (under
   `lib/auv/pipewire-0.3` and `lib/auv/spa-0.2`) and `client.conf` itself
   (under `share/auv/pipewire`). At startup, `auv` points
   `PIPEWIRE_MODULE_DIR`, `SPA_PLUGIN_DIR`, and `PIPEWIRE_CONFIG_DIR` at them
   unless the user set those variables, and removes the injected values again
   before starting provider Runners and plugins. The host PipeWire daemon and
   its socket protocol are the boundary, as they are for Flatpak runtimes. A
   release check connects the bundled client to each verified distribution's
   own daemon. (Amended 2026-10-22.)

The PipeWire amendment comes from a 2026-10-22 check of the arm64 bundle. The
bundled `libpipewire` 0.3.65 loaded the host's client modules. On Debian 13
(PipeWire 1.4.2) and Ubuntu 26.04 (1.6.2), those modules need
`pw_log_topic_register`, which 0.3.65 lacks, so `pw_context_new` failed and
screen capture could not start. Only Debian 12 worked. `auv --help` and OCR
passed everywhere, so the original checks did not catch it. With Debian 12's
client modules, plugins, and configuration selected through the three
variables, the same bundled library connected to the Ubuntu 26.04 1.6.2
daemon. Treating `libpipewire` as a host library was rejected because `auv`
would then fail to start on hosts without PipeWire; loading it on demand is a
possible later change that needs its own slice.

A throwaway 0.0.29 prototype copied the non-glibc dependency closure, patched
application-relative RUNPATH values, and added `eng.traineddata`. It started on
clean Debian 13 without OCR packages. The compressed archive was approximately
41 MB (approximately 115 MB unpacked). The same prototype failed on Debian 12
because the Ubuntu 24.04 executable required GLIBC 2.39 and bundled libraries
required GLIBC 2.38. A subsequent real build on Ubuntu 22.04 failed because its
PipeWire 0.3.48 development headers do not provide the API expected by
`pipewire-rs` 0.8. Debian 12 supplies the required PipeWire 0.3.65 headers and
sets the resulting host ABI boundary at GLIBC 2.36. These results are why
library bundling and the Debian 12 build baseline are one inseparable release
change.

## Deferred channels

A future `auv-minimal` artifact may externalize native dependencies and document
the corresponding distro packages. It is intentionally not part of the default
artifact and must not replace the full bundle without an owner-approved channel
contract.

`.deb`, `.rpm`, Flatpak, AppImage, musl, and service-installation artifacts are
also deferred. When those channels are added, their manifests should own their
dependencies in the channel-native way: distro packages should declare
Tesseract plus the English language package, while Flatpak should build the OCR
stack and trained data into its application filesystem. Each channel needs its
own Portal, socket, daemon, and host-integration acceptance evidence; the full
tarball does not establish those claims.
