# Tessera — Engineering Notes

## Design System

Always read `DESIGN.md` before making visual or UI decisions. All font choices,
colours, spacing, motion, and aesthetic direction are defined there. Do not
deviate without explicit user approval. Flag any code that doesn't match
`DESIGN.md` during review.

Quick reference (full details in `DESIGN.md`):
- Typography: **Geist** (UI) and **Geist Mono** (paths, branches, hashes).
- Palette: warm dark — `--bg #0F0F10`, accent terracotta `--accent #C8825B`.
- Compact density, 4px base unit.
- Motion: minimal. Signature exception: `working` status dot breathes 1.6s.

## Authorship

This is `nvrxq/Tessera`. All commits MUST be authored as
`nvrxq <nvrxq@users.noreply.github.com>`. Use:

```bash
git -c user.name=nvrxq -c user.email=nvrxq@users.noreply.github.com commit ...
```

Never use the `gh` CLI. Push via raw HTTPS with the token at `/home/save/nvr_tok`.
