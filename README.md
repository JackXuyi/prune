# Prune

**English** · [简体中文](./README.zh-CN.md)

### Your Mac is holding files it can never load. Prune finds them.

A free, MIT-licensed desktop app for developer machines: it finds **disk space you can actually reclaim** — cross-platform binaries, duplicate package-manager stores, whitelisted dev caches, Xcode and simulator debris — and it reports **memory the way the OS accounts for it**, including everything that has been swapped out to disk.

**Prune is not a memory monitor and not a general system cleaner.** It is a waste finder: it names what has been wasted, shows the evidence, and leaves the decision to you. It reads no sensors, keeps no cloud account, and has no "optimize" button.

Tauri 2 · Rust · React 19 · no account · no telemetry · no elevated permissions.

[![release](https://img.shields.io/github/v/release/JackXuyi/prune?label=release&color=blue)](https://github.com/JackXuyi/prune/releases/latest)
[![platform](https://img.shields.io/badge/platform-macOS%2013%2B%20%C2%B7%20Windows%2010%2B%20beta-black)](#install)
[![license](https://img.shields.io/github/license/JackXuyi/prune?color=green)](./LICENSE)
[![stack](https://img.shields.io/badge/stack-Tauri%202%20%C2%B7%20Rust%20%C2%B7%20React%2019-informational)](#build-from-source)
[![telemetry](https://img.shields.io/badge/telemetry-zero-brightgreen)](#agents-read-humans-delete)
[![downloads](https://img.shields.io/github/downloads/JackXuyi/prune/total?label=downloads)](https://github.com/JackXuyi/prune/releases)

---

## Contents

- [Why this exists](#why-this-exists)
- [Who it is for](#who-it-is-for)
- [Platform support](#platform-support)
- [What Prune does](#what-prune-does)
- [The six tabs](#the-six-tabs)
- [Menu bar: the number that matters](#menu-bar-the-number-that-matters)
- [Cleanup: three levels, four guardrails](#cleanup-three-levels-four-guardrails)
- [Memory and apps](#memory-and-apps)
- [Network, ports, LAN](#network-ports-lan)
- [Event stream: the Dynamic Island](#event-stream-the-dynamic-island)
- [First two minutes](#first-two-minutes)
- [CLI](#cli)
- [MCP server](#mcp-server)
- [Agents read, humans delete](#agents-read-humans-delete)
- [Install](#install)
- [Build from source](#build-from-source)
- [Status and roadmap](#status-and-roadmap)
- [Docs](#docs)
- [Keywords](#keywords)
- [License](#license)

## Why this exists

Three measurements taken on one real developer Mac (2026-10, sample size n = 1 — expect different numbers on your machine, but the same three mechanisms):

| Measurement | Number |
| --- | --- |
| Yarn cache holding **334 cross-platform binary packages** (linux 196 / win32 74 / android 25) that an arm64 machine will never load | **7.74 GB**, waste ratio 8.3 : 1 |
| npm + Yarn + pnpm storing **the same dependencies three times over** | **29.25 GB** |
| Activity Monitor showing `next-server` at **113 MB**, while its real footprint was **3432 MB** (97% swapped to disk) | RSS was lying |

System monitors do not look at disk. Cleaners do not understand package-manager caches. Prune sits in between — and it reports `phys_footprint` instead of RSS, so the memory number matches what the machine actually pays.

The same product-rule ran through everything afterwards: **report what is verifiable, delete only what is provably regenerable, and never decide on the user's behalf.**

## Who it is for

**Prune is for you if** you develop on a 256–512 GB machine, you keep AI tools and Node projects running all day, you have seen "Your startup disk is almost full", and you would like to know which cache is 8 GB before deleting one.

**Prune is probably not for you if** you want a "clean everything automatically" button (Prune has never shipped one), a general hardware monitor (Prune reads no sensors at all), full-disk scanning, photo dedupe, malware scanning, or fleet telemetry dashboards.

## Platform support

Prune is macOS-first, not macOS-only: one Rust core, three delivery shapes.

| Platform | GUI | CLI + MCP | Status |
| --- | --- | --- | --- |
| **macOS 13+** (Apple Silicon / Intel) | `.dmg` | ✓ | Reference platform. Data sources: `footprint`, `lsof`, `nettop`, `networkQuality` |
| **Windows 10+ x64** | `.exe` / `.msi` | builds from source (`scripts/check-windows.sh` cross-compiles and links it) | **Beta** — code complete and installers ship on every release; the real-device checklist (tray position, dark taskbar, Temp deletion) has not been walked yet |
| **Linux** | — | `prune-core` + `prune-cli` compile and are covered by the test suite in CI (ubuntu runner) | No build is published and no Linux desktop has been walked through — source-only |

### What differs on Windows

Nothing below is simulated: a field with no equivalent is hidden, never faked.

| Capability | macOS | Windows |
| --- | --- | --- |
| Cache whitelist | 52 entries active (40 macOS-only + 12 shared) | 28 entries active (16 Windows-only + 12 shared) |
| Cross-platform binary detection (A1) | Yarn v6 — the directory name carries the platform | not applied; Yarn stays out of A1 until it is measured on Windows |
| Duplicate npm / Yarn / pnpm stores | ✓ | ✓ (`%LOCALAPPDATA%` layout) |
| Cleanup (whitelist + Trash + 20 GB breaker), project health, archive | ✓ | ✓ |
| Real process memory | `phys_footprint`, swapped bytes included | Private Bytes — an approximation that **excludes** the swapped-out part |
| Swap gauge | total / used + swapfile count + new today | total / used only |
| Memory pressure | kernel pressure level | `dwMemoryLoad` occupancy bands (< 80 / 80–91 / ≥ 92) |
| Evicted-victim ranking | ✓ | hidden (no swapped figure exists to rank by) |
| Leak case files | ✓ | ✓ (on Private Bytes) |
| Project attribution for a process | handle + tag + inheritance | tag + inheritance (no `lsof` equivalent) |
| Per-app disk layers (bundle / cache / data) | ✓ | not available — all three resolve from `.app` + `~/Library` paths |
| Per-app network speed, per-app Top 8 | `nettop`, 1 s diff | unavailable — needs an ETW real-time session |
| System throughput + 5-min trend + daily totals | ✓ | ✓ (`sysinfo` counters) |
| Network diagnosis (DNS / gateway / egress) | ✓ + system speed test & RPM | diagnosis only; the speed-test card hides itself |
| Port reverse lookup | `lsof` — unprivileged runs lose other users' processes, and `coverage` says how many | `GetExtendedTcpTable` — complete without admin, but the "which project" column stays empty |
| LAN devices / timeline / topology | `arp -an` + `lsof` peers | `GetIpNetTable2` + `GetExtendedTcpTable` |
| Event stream (Dynamic Island) | native panel in the notch | not implemented — the island commands exist as no-ops |
| Tray icon animation + pet moods | template icon, system inverts it | colour frames (a black template icon is invisible on a dark taskbar) |
| Menu bar panel | native popover | tray-anchored window with a 300 ms focus grace period |
| UI language (中文 / English) | ✓ | ✓ |

Windows is ahead in one place: port lookup needs no admin and misses nothing, while macOS hides other users' listeners. Everywhere else the gap is a missing OS facility, not a missing feature.

## What Prune does

| Does | Does not |
| --- | --- |
| Detect caches your machine **can never load** (cross-platform binaries); every deletion stays inside a fixed whitelist and lands in the Trash | General hardware monitoring |
| Attribute duplicate storage across npm / Yarn / pnpm | "Free up memory to make your Mac faster" |
| Report **real** process memory (`phys_footprint`, swapped bytes included) — swap is an active mechanism, not junk | Full-disk scanning, documents, photo dedupe |
| Swap dashboard: how much disk memory pressure is eating, and who got evicted | Any deletion outside a fixed whitelist |
| Leak *suspicion* as a case file (evidence + curve + conservative wording) | Claiming a verdict it cannot prove |
| Port reverse lookup: who owns 3000 / 3118, and which project it belongs to | Firewalls, packet capture, per-domain traffic |
| Per-app resource cards: processes / CPU / memory / disk / ports / connections / speed | GPU and energy metrics |
| Network tab: system-wide throughput, 5-minute trend, per-interface split, per-app Top 8, today / last 7 days | Rate limiting, disconnecting, killing connections |
| Network diagnosis: DNS / gateway / egress latency, captive portal, one verdict with published thresholds, Markdown report | Wi-Fi signal, DNS switching (writes system config), channel scanning |
| Dynamic Island events: CPU spike, memory anomaly, port conflict, new LAN face | Always-on widgets, music control, file drop |

Some rows are macOS-only today (per-app network speed, per-app disk layers, the event stream), and the memory rows read Private Bytes instead of `phys_footprint` on Windows. [Platform support](#platform-support) lists exactly which.

## The six tabs

| Tab | What it answers |
| --- | --- |
| **Overview** (default) | Reclaimable GB, memory / CPU / disk, listening ports, download & upload — six read-only cards, all navigational |
| **Apps** | One card per app: processes, CPU, memory, disk, ports, connections, network speed. Top 12 by default, six-field sorting, 24 h / 7 d / 30 d history |
| **Memory** | Real footprint, swap gauge, evicted-victim ranking, process families, leak case files |
| **Storage** | Volume capacity, reclaimable breakdown by level, per-entry detail, cleanup, project health, archive |
| **Network** | Download / upload rates (physical interfaces only), trends, per-app Top 8, daily traffic, network diagnosis |
| **Ports** | Who is listening, on which address, owned by which project; LAN devices, timeline, topology |

Everything is read-only except the Storage tab's cleanup actions and the archive/restore flow — and those are the only two places where a file can move at all.

## Menu bar: the number that matters

```
menu bar  ⚡ 54.9 GB reclaimable        pressure ● green
├──────────────────────────────────────────┤
│ 1. Memory                                │
│   ⚠ swap 14.0 GB (93% of cap)            │
│   ↳ eating 16 GB of disk · +6 today      │
│   ▸ victims: next-server 3.2 GB (97%)    │
│ 2. Reclaimable                           │
│   🟢  7.74 GB  cross-platform binaries   │
│   🟢 22.5 GB   package-manager caches    │
│   🟡 21.7 GB   project deps / simulators │
│   🔴  3.0 GB   Docker (command only)     │
└──────────────────────────────────────────┘
```

The headline number is **reclaimable GB**, not a memory percentage: a percentage describes, gigabytes are actionable. The menu bar panel (380 × 432) renders the same readings as the main window from one shared formatting layer (`lib/statsReadout.ts`), so the two screens can never disagree.

The tray icon is also a reading: the scissors' cutting speed (or the cat's jump rate) encodes CPU busy-ness, and the tooltip carries the exact value (`Prune · CPU 12%`).

## Cleanup: three levels, four guardrails

**68 registered cache locations** — 53 are one-click safe (🟢), 12 need per-item confirmation (🟡: project deps, simulators, model weights), 3 are command-only and never executed (🔴: Docker VM, OrbStack, Colima).

| Guardrail | Implementation |
| --- | --- |
| Path whitelist | Cleanup accepts only registered paths; free-form input is rejected in `prune-core` |
| Trash, never `rm -rf` | Every deletion goes through the system trash (`trash` crate) and is recoverable |
| Dry-run first | First execution lists the manifest before touching anything |
| Byte breaker | A single run above 20 GB requires a second explicit confirmation, even at 🟢 |

Prune never asks for root, full disk access, or accessibility permissions. The one macOS privacy prompt ("local network") appears only when you actively run a LAN scan.

**Project health** turns "N node_modules at X GB" into a per-project ledger: dependency size, last install, last project activity, competing lockfiles. Verdicts are stated as facts (dormant ≥ 60 days, heavy ≥ 1.5 GB) and never auto-deleted.

**Archive** is the reversible alternative: a dormant project's `node_modules` is packed to `tar.gz` in a managed archive, then re-read and verified (entry count + byte count) — **only if verification passes is the original moved to the trash**. Restore previews conflicts first, and user code is never archived.

## Memory and apps

- Data source is the system's own `/usr/bin/footprint -p <pid> -j` — non-privileged, JSON, with a swapped breakdown. No Mach API, no entitlements. On Windows the basis is `GetProcessMemoryInfo` private bytes — an approximation of the macOS footprint number, not an equivalent of it, because it cannot see the swapped-out part; fields that cannot be read (swapped bytes, the victim ranking) are not rendered at all.
- Sampling: a cheap RSS sweep every 10 s, a deep footprint pass over the top N every 5 minutes (8 concurrent ≈ 0.6 s).
- **Leak suspicion as a case file**: a case is opened (`CASE-MMDD-NN`), evidence accumulates (growth curve + numbers), and it closes itself when the suspect leaves and does not reappear. The wording rule is fixed: suspicion, never a verdict.
- Unreadable values are shown as "unavailable + reason", never as 0 — 0 and "could not read" are different facts.

## Network, ports, LAN

**Network** counts physical interfaces only (`utun` / `awdl` are listed separately and excluded from totals, so a VPN cannot double-count), plus per-app Top 8 via `nettop` (macOS only), today's and last-7-days traffic persisted to SQLite, and a diagnosis pass that measures real TCP connect latency to DNS, gateway and egress, checks for a captive portal, and prints one verdict with every threshold shown (50 / 100 / 30 / 300 ms). One click copies a Markdown report. macOS adds the system speed test with RPM (bufferbloat); Windows has no equivalent, so that card hides itself.

**Ports** answers "who owns 3000" with the owning process, bind address, project directory and established connections — plus `coverage`, which tells you how many listeners were invisible to an unprivileged process. "Not found" is never reported as "free". Service fingerprinting labels each listener with an *inferred* or *confirmed* badge (static inference first; the active probe is opt-in).

**LAN** has two speeds. The background event stream is an observer: it reads the neighbour table (60 s cadence, no probing, no `lsof`). The LAN tab's scan is your explicit action, and only then does it probe the subnet. Either way it keeps a stable-MAC identity across sessions, shows first-seen / last-seen, draws a topology view, and raises one event when a face appears that has never been seen before.

## Event stream: the Dynamic Island

Four event types can grow out of the notch, each with its own rate limit and cooldown:

1. **CPU spike** — sustained burn, with hysteresis so a single compile does not fire it
2. **Memory anomaly** — kernel pressure critical, or swap over threshold
3. **Port conflict** — a watched port goes from free to taken, or gains a second holder
4. **New LAN face** — an unseen device appears on the subnet

The collapsed state is icon + label + reading; hovering expands to show *who* is burning CPU or *who* got swapped out. It dismisses itself after 6 s, never takes focus, and never sits there as a widget.

**macOS only.** The panel is an `NSPanel` positioned against the notch; on Windows the island commands are no-ops, so no event ever fires.

## First two minutes

1. Launch. The storage tab starts scanning immediately — no five-step onboarding, no sign-up.
2. The first scan ends on a card that reports the 🟢 total only.
3. One click reclaims it: dry-run → whitelist → trash, all on 🟢 entries.
4. A receipt appears: what this run removed and what has been removed cumulatively.

There is no account, no network call, and no "rate your experience" prompt. The only optional action on that screen copies six numbers to the clipboard so you can paste them into a bug report or an interview.

## CLI

One binary, two shapes: `cargo build --release -p prune-cli` produces `target/release/prune`, sharing `prune-core` with the GUI.

```bash
prune scan [--json]                 # full disk scan: headline / cross-platform detail / duplicate stores / Docker hints
prune reclaim [--level green]       # preview reclaimable items (dry-run by default; --yes deletes, via trash)
prune mem [--swapped] [--json]      # one-shot memory reading (footprint, deep probe top 20)
prune mem --watch [--interval N]    # continuous sampling with a leak-suspicion verdict every 60 s
prune port [PORT] [--json] [--all]  # who owns this port (no PORT: watched layer; --all: internal IPC)
prune app <family> [--json]         # one app's resource profile: processes/memory/CPU/ports/conns/disk
prune lan [--scan] [--json]         # LAN devices (default: cache only; --scan probes the subnet)
prune mcp                           # MCP server over stdio (agent query entry point)
```

## MCP server

**MCP tools (8, all read-only):** `mem_snapshot` (one-shot memory reading) / `mem_leaks` (leak suspicion from the GUI's series store) / `disk_report` (full disk scan, ~15 s) / `disk_scan` (per-entry whitelist detail with optional child split, sub-second) / `port_listeners` (port reverse lookup, with `coverage` explaining what was visible) / `net_diag` (exposure verdict: who is listening on all interfaces) / `app_resources` (five-domain app profile) / `lan_devices` (LAN devices, cache only, sends no packets).

Wire it into any MCP-capable agent and ask questions like "what is eating disk on this machine", "is anything of mine exposed on the LAN", or "is this process leaking" — the agent reads, you decide.

## Agents read, humans delete

This is a code-level property, not a slogan, and it is covered by tests:

1. **Agents can look.** All 8 MCP tools are queries. There is no execution tool.
2. **Only a human can delete.** Cleanup happens in the GUI, behind whitelist + trash + 20 GB breaker. There is no IPC command an agent could call to delete anything.
3. **The dispatcher refuses by name.** `reclaim` / `clean` / `execute` / `delete` and friends return an explicit read-only error that says why — not a generic "unknown tool". A test asserts that arm exists.
4. **Whitelist as the last line.** Even if an execution path were opened later, only registered regenerable paths could be touched; free-form paths die in `prune-core`.

Plus the product-level commitment: **zero telemetry**. No analytics SDK, no phone-home, no URL with a device id. The activation numbers Prune records stay in a local SQLite file and leave the machine only if you copy them.

## Install

Download the latest build from [Releases](https://github.com/JackXuyi/prune/releases/latest):

| Platform | File |
| --- | --- |
| macOS Apple Silicon (M1 and later) | `Prune_*_aarch64.dmg` |
| macOS Intel | `Prune_*_x64.dmg` |
| Windows 10+ x64 (beta) | `Prune_*_x64-setup.exe` (recommended) or `Prune_*_x64_en-US.msi` |

**macOS, three steps**

1. Open the dmg and drag **Prune** into **Applications**.
2. Launch it. If macOS says it "cannot be opened because the developer cannot be verified", go to **System Settings → Privacy & Security**, scroll to the bottom, and click **Open Anyway** (once).
3. If that button is missing on your macOS version, run once in Terminal:
   `xattr -dr com.apple.quarantine /Applications/Prune.app`

**Windows**: run `Prune_*_x64-setup.exe`; if SmartScreen warns you, choose **More info → Run anyway**.

**Linux**: no GUI build is published. `cargo build --release -p prune-cli` gives you the CLI/MCP binary — see [Platform support](#platform-support) for what it can and cannot read there.

**Why this is needed**: the build is ad-hoc signed — no paid Apple Developer account, no notarization — so Gatekeeper does not recognize it. That is the entire reason for the prompt. The bundles are complete, and in-app updates do not use this path (the updater verifies its own signatures), so upgrading from the tray menu will not show this again. This is not a disclaimer: it is the fact, and the choice is yours.

## Build from source

```bash
pnpm install
pnpm tauri dev      # development
pnpm tauri build    # package .app / .dmg
cargo build --release -p prune-cli   # CLI + MCP binary
cargo test --workspace               # Rust test suite
pnpm check:docs                      # docs honesty gate (MCP tool reconciliation + public-claim sweep)
```

Requirements: Node 20+, pnpm 9+, Rust stable, and the Tauri 2 platform prerequisites.

## Status and roadmap

Source version: **v0.10.0** — free, MIT, no commercial plan for the next two quarters. The badge at the top tracks the newest **published** build on the Releases page.

| Area | State |
| --- | --- |
| Disk waste detection + tiered cleanup | Shipped (A1 cross-platform binaries, duplicate stores, 68 whitelist entries, project health, archive) |
| Memory truth, swap, leak case files | Shipped (footprint basis, victim ranking, case files) |
| App / network / port / LAN tabs | Shipped (per-app 30-day history, network diagnosis, LAN timeline + topology) |
| Event stream (Dynamic Island) | Shipped (4 event types, configurable thresholds) |
| CLI + read-only MCP server | Shipped (8 tools, execution requests refused by name) |
| Activation loop | Shipped in 0.10.0 (first scan → one click → receipt; local-only counters) |
| Windows | **Beta**: installers ship every release, core features work; the missing pieces are listed under [Platform support](#platform-support); no further port schedule committed |
| Auto-cleanup, telemetry, cloud accounts | Not planned |

The full milestone history lives in [CHANGELOG.md](./CHANGELOG.md); the forward plan (M1–M8, gates, death lines, explicit non-goals) is in [docs/prune-roadmap-2027h1.md](./docs/prune-roadmap-2027h1.md).

## Docs

- [Product spec v2](./docs/prune-feature-spec-v2.md) — positioning, modules, safety levels
- [Tauri technical plan](./docs/prune-tauri-technical-plan.md) — architecture, dependencies, milestones
- [Roadmap 2027 H1](./docs/prune-roadmap-2027h1.md) — milestones, metric gates, what is explicitly not being built
- [Network diagnosis plan](./docs/net-diag-plan.md) — layered probes, verdict rules, dual-platform adaptation
- [Research archive](./docs/research/) — competitor teardowns, measured scans, memory profiling, decisions
- [CHANGELOG](./CHANGELOG.md) — every shipped milestone, including the ones that were bugs

## Keywords

`macOS disk space analyzer` · `developer cache cleaner` · `node_modules size` · `yarn cache` · `pnpm store` · `npm cache` · `Xcode DerivedData` · `iOS simulator cleanup` · `AI CLI cache` (Cursor / Claude / Codex / Ollama / HuggingFace / LM Studio) · `swap usage monitor` · `phys_footprint` · `memory leak detection` · `who is using port 3000` · `LAN device discovery` · `MCP server` · `Tauri` · `Rust` · `Apple Silicon` · `Windows` · `cross-platform` · `zero telemetry` · `open source system utility`

## License

[MIT](./LICENSE). The whitelist is a data file on purpose: cache locations change faster than release cycles, and a wrong path is a data-loss incident, so every entry ships with an assertion test.
