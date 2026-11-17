# Project record

## Objective

Terminal log viewer core that tails, pages and filters multi-gigabyte files through bounded reads, with level highlighting and a follow mode that survives truncation

## Scope

- In scope: the public API documented in [README.md](../README.md), documented and tested.
- Out of scope: anything that needs dependencies or I/O without an accepted ADR.

## Timeline

| Date | Event |
|---|---|
| 2026 | Repository created from the factory utility template. |

## Effort metrics

Measured values only; a value that has not been measured stays "pending measurement".

| Metric | Value |
|---|---|
| Tokens (input / output) | 1443155 / 32129 |
| Hours (human) | pending measurement |
| Cost (USD) | 0.8494 |

## Factory run u939 (measured from API usage)

| Date (UTC) | Phase | Model | Batch | Input | Output | Cache write | Cache read | Cost (USD) |
|---|---|---|---|---|---|---|---|---|
| 2026-11-17 | generation | google/gemini-4-argon | yes | 6183 | 26822 | 0 | 0 | 0.5612 |
| 2026-11-17 | fix-1 | google/gemini-3.8-flash | no | 71688 | 2612 | 0 | 543618 | 0.1043 |
| 2026-11-17 | fix-2 | google/gemini-3.8-flash | no | 52261 | 250 | 0 | 170242 | 0.0529 |
| 2026-11-17 | audit | google/gemini-3.8-flash | no | 112791 | 897 | 0 | 462248 | 0.1226 |
| 2026-11-17 | docs | google/gemini-3.1-flash-lite | no | 24124 | 1548 | 0 | 0 | 0.0084 |

Total: $0.8494. Prices: vertex line, as measured by its adapter. Providers serve the current version of each model id, so the date tells which version answered; versions, launch stages and price sources per line are in the factory's model register (docs/MODELS.md).
