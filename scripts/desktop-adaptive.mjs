// Reuses the existing real UI/WGC/parallel lifecycle regression, then exercises Adaptive.
process.env.GAMEOCR_TEST_ADAPTIVE='1';
process.env.GAMEOCR_PARALLEL_DESKTOP_REPORT??='docs/test-results/desktop-adaptive.json';
await import('./desktop-parallel.mjs');
