## What and why

<!-- What does this change, and why? Link the issue it fixes (Fixes #123). -->

## How it was tested

<!--
Say exactly what you ran and saw. Bluetooth behaviour can only be confirmed with a real phone:
if you didn't try it on one, say "not run against a phone". Never claim hardware verification
you didn't do.
-->

- [ ] `npm run check` passes locally
- [ ] Tried in the browser preview (`npm run dev`) — which scenario(s): <!-- e.g. ?call, ?devtools -->
- [ ] Tried on real hardware — phone model / iOS: <!-- or "not run against a phone" -->

## Checklist

- [ ] Logic lives in a pure module (`ancs.rs`, `ams.rs`, `store.rs`, `link_policy.rs`, `src/lib/*.ts`, …) with tests; Bluetooth/WinRT code stays thin I/O
- [ ] Rust serde types and `src/types/protocol.ts` changed together (if either changed)
- [ ] UI copy is end-user wording, "tug" in lowercase, no developer or test details
- [ ] Colours, fonts and spacing use the theme tokens in `src/style.css` (no inline hex)
- [ ] No personal data in code, fixtures, screenshots or logs (555-01xx numbers, `example.com`)
- [ ] User-visible change: CHANGELOG.md updated (and `src/lib/whatsNew.ts` for a release)
- [ ] Commit messages explain why
