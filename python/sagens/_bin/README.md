`xtask` stages the signed `sagens` host binary here for wheel and local package builds:

```bash
rtk cargo run --bin xtask -- dev --python-package-root python
```

The binary itself is a build artifact and is intentionally ignored by git.
