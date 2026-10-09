# D07 regression continuation

Continues [D07.md](D07.md), same approved W1/D07 plan and feature branch.

CI #149 / 35719852182 tested head 8f03d0f9bf72f9e432e4f2f9a129bd46356153ca. Both full logs were read:
- All 11 new D07 frontend tests passed. The full frontend suite had 619 passed / 3 failed, plus three unhandled rejections. The old app-store fixtures returned null/undefined for the newly required cli_register_project call.
- Rust library: 363 passed / 0 failed / 14 ignored. Main: 6 passed. Launch configuration: 2 passed / 3 ignored. Transport: 8 passed. Clippy passed. Three rustfmt differences remained.

Ruling: acknowledge the registration call in the old app-store mocks while retaining the real API facade and every existing cwd rollback and write-order assertion. The fast A/B test now waits for A to enter persistence instead of assuming one microtask crosses the additional registration await. This corrects fixtures and strengthens ordering evidence; it does not disable receipt validation or suppress unhandled errors.
Ruling: apply the three exact rustfmt changes reported by CI #149; no backend behavior is changed in this follow-up.

The legacy setCwd fire-and-forget path still does not present persistence failures. Its migration/error handling remains an explicit W7/D24 integration item; the new picker and awaited setCurrentProject paths do not establish acceptance for that legacy path. Full project-tree consumption, startup check isolation and interactive host certification remain as listed in D07.md.

Final acceptance must use the next exact commit's complete CI, not the partial results above. No main write, merge, tag, release, dependency upgrade or native CLI configuration change.
