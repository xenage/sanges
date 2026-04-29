# @xenage/sanges

Node SDK for giving each agent its own durable sagens BOX.

```js
import { Daemon } from "@xenage/sanges";

const daemon = await Daemon.start();
try {
  const box = await daemon.createBox();
  await box.start();
  await box.fs.write("/workspace/message.txt", Buffer.from("hello"));
  const result = await box.execBash("cat /workspace/message.txt");
  console.log(result.stdoutText);
} finally {
  await daemon.close();
}
```

The package installs a small SDK plus a platform-specific host binary through
npm optional dependencies. No source build or runtime asset download is needed.

## Support

- Host OS and CPU: macOS `arm64`, Linux `x64`, Linux `arm64`
- Node versions: `>=20`
- Linux full microVM runtime requires `/dev/kvm`
- The current libkrun-only backend does not support macOS `x64` or Windows hosts
