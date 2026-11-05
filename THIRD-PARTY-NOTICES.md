# Third-party notices / Üçüncü taraf bildirimleri

CarlUI itself is licensed under the MIT License (see `LICENSE`). It includes or
links the following third-party components, which remain under their own
licenses.

CarlUI'nin kendisi MIT lisanslıdır (`LICENSE`). Aşağıdaki parçalar kendi
lisanslarıyla gelir.

## PawnIO IntelMSR module — LGPL-2.1

`carlui-sensors.exe` embeds `IntelMSR.bin`, a signed module of the PawnIO
project, used to read Intel CPU temperatures. It is distributed unmodified under
the GNU Lesser General Public License, version 2.1. The full license text is in
`crates/carlui-sensors/IntelMSR.COPYING.txt` (installed as
`IntelMSR.COPYING.txt`). Source code: https://github.com/namazso/PawnIO.Modules

The PawnIO kernel driver itself is **not** bundled; users install it separately
(https://pawnio.eu). Without it, CarlUI runs normally and simply shows no CPU
temperature.

`carlui-sensors.exe`, Intel CPU sıcaklığını okumak için PawnIO projesinin
imzalı `IntelMSR.bin` modülünü değiştirilmeden içerir (LGPL-2.1). PawnIO
sürücüsü kurucuda yoktur; kullanıcı isterse ayrıca kurar.

## Rust crates

All of the following are used under the MIT license (most are dual-licensed
"MIT OR Apache-2.0"; CarlUI uses them under MIT).

| Crate | License | Source |
|---|---|---|
| windows, windows-core, windows-implement, windows-interface, windows-result, windows-strings, windows-targets, windows_x86_64_msvc | MIT OR Apache-2.0 | https://github.com/microsoft/windows-rs |
| serde, serde_core, serde_derive | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_json | MIT OR Apache-2.0 | https://github.com/serde-rs/json |
| toml, toml_edit, toml_datetime, toml_write, serde_spanned | MIT OR Apache-2.0 | https://github.com/toml-rs/toml |
| winnow | MIT | https://github.com/winnow-rs/winnow |
| indexmap, equivalent | Apache-2.0 OR MIT | https://github.com/indexmap-rs/indexmap |
| hashbrown | MIT OR Apache-2.0 | https://github.com/rust-lang/hashbrown |
| itoa, zmij | MIT OR Apache-2.0 / MIT | https://github.com/dtolnay |
| memchr | Unlicense OR MIT | https://github.com/BurntSushi/memchr |
| rustfft, realfft, easyfft, transpose, strength_reduce, primal-check (used by nnnoiseless) | MIT OR Apache-2.0 / MIT | https://github.com/ejmahler/RustFFT |
| num-complex, num-integer, num-traits | MIT OR Apache-2.0 | https://github.com/rust-num |
| once_cell, smallvec, parking_lot, parking_lot_core, lock_api, scopeguard, cfg-if, array-init, generic_singleton | MIT OR Apache-2.0 | crates.io |
| anymap3 | BlueOak-1.0.0 OR MIT OR Apache-2.0 | https://github.com/reivilibre/anymap3 |
| ruzstd (setup only) | MIT | https://github.com/KillingSpark/zstd-rs |
| zstd, zstd-safe, zstd-sys (build time only, compresses the setup payload) | MIT / BSD-3-Clause | https://github.com/gyscos/zstd-rs |
| proc-macro2, quote, syn, unicode-ident (build time only) | MIT OR Apache-2.0 (unicode-ident also Unicode-3.0) | https://github.com/dtolnay |

### MIT License (applies to the crates above)

Permission is hereby granted, free of charge, to any person obtaining a copy of
this software and associated documentation files (the "Software"), to deal in
the Software without restriction, including without limitation the rights to
use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of
the Software, and to permit persons to whom the Software is furnished to do so,
subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS
FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN
AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## nnnoiseless / RNNoise — BSD-3-Clause

`carlui-replay.exe` uses nnnoiseless (a Rust port of RNNoise) for microphone
noise suppression. Source: https://github.com/jneem/nnnoiseless

Anlık tekrardaki mikrofon gürültü engellemesi nnnoiseless (RNNoise) kullanır.

```
Copyright (c) 2020, Joe Neeman
Copyright (c) 2017, Mozilla
Copyright (c) 2007-2017, Jean-Marc Valin
Copyright (c) 2005-2017, Xiph.Org Foundation
Copyright (c) 2003-2004, Mark Borgerding

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions
are met:

- Redistributions of source code must retain the above copyright
notice, this list of conditions and the following disclaimer.

- Redistributions in binary form must reproduce the above copyright
notice, this list of conditions and the following disclaimer in the
documentation and/or other materials provided with the distribution.

- Neither the name of the Xiph.Org Foundation nor the names of its
contributors may be used to endorse or promote products derived from
this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
``AS IS'' AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
A PARTICULAR PURPOSE ARE DISCLAIMED.  IN NO EVENT SHALL THE FOUNDATION
OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## Fonts and icons

CarlUI draws its icons with the Segoe Fluent Icons / Segoe UI fonts that ship
with Windows; no font files are bundled. The CarlUI application icons in
`assets/icons` are part of this project (MIT).
