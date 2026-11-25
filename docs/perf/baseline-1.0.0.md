# Libellus performance baseline 1.0.0

Release build. Same harness for the baseline and later runs.

## Machine

- Linux 6.12.113+ #1 SMP PREEMPT_DYNAMIC Mon Nov 23 16:05:51 UTC 2026 x86_64
- rustc 1.83.0 (90b35a623 2024-11-26)
- model name : Intel(R) Xeon(R) Processor
- CPUs: 4
- MemTotal:       16387328 kB
- host triple: x86_64-unknown-linux-gnu

Median is the upper middle sample. p95 is the nearest rank, ceil(0.95 * n) - 1.
Times are microseconds of wall time unless a row says otherwise.

## Timings

| Path | Runs | Median us | p95 us |
|---|---:|---:|---:|
| note write (growing to 10) | 10 | 477 | 2146 |
| ls limit 50 at 10 notes | 15 | 47 | 95 |
| get one note at 10 notes | 15 | 5 | 11 |
| brief at 10 notes, no ledger yet | 15 | 36 | 43 |
| note write (growing to 1000) | 990 | 181 | 487 |
| ls limit 50 at 1000 notes | 15 | 3329 | 5600 |
| get one note at 1000 notes | 15 | 5 | 14 |
| brief at 1000 notes, no ledger yet | 15 | 3300 | 3600 |
| note write (growing to 10000) | 9000 | 159 | 225 |
| ls limit 50 at 10000 notes | 5 | 43373 | 58492 |
| get one note at 10000 notes | 5 | 6 | 25 |
| brief at 10000 notes, no ledger yet | 5 | 40044 | 41006 |
| spend append, entries 0 to 9 | 10 | 547 | 5994 |
| spend append, entries 10 to 999 | 990 | 1780 | 3076 |
| spend append with 10000 entries already stored | 20 | 20240 | 21385 |
| ledger read at 10000 entries | 10 | 9434 | 9538 |
| brief at 10000 notes and 10000 ledger entries | 8 | 54169 | 56381 |
| CLI cold start, libellus --version | 30 | 645 | 1112 |
| CLI cold brief at 10000 notes | 8 | 61836 | 65492 |
| MCP cold start plus one brief | 8 | 65499 | 71404 |
| MCP warm brief round trip | 30 | 51567 | 66815 |
| read_strict fixture 262144 bytes | 10 | 17 | 254 |
| read_strict fixture 4194304 bytes | 10 | 615 | 1880 |
| get note with 40 evidence pointers and a long body | 20 | 12 | 19 |

## Store size

- at 10 notes: notes directory 1380 bytes, ledger file 0 bytes, store total 1380 bytes
- at 1000 notes: notes directory 141780 bytes, ledger file 0 bytes, store total 141780 bytes
- at 10000 notes: notes directory 1437780 bytes, ledger file 0 bytes, store total 1437780 bytes
- at 10000 notes and the ledger: notes directory 1437780 bytes, ledger file 1222440 bytes, store total 2660220 bytes

## Large fixtures

- fixture 262144 bytes on disk: 262144 bytes
- fixture 4194304 bytes on disk: 4194304 bytes
- large-body note file: 15705 bytes

Note writes and spends go through the same tool handlers as MCP.
The 10000-entry spend sample starts from a ledger written in one go, then times real appends.
The smaller spend rows are the real appends that grew the file to 1000 entries.
