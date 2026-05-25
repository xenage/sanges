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

## Named Images

See [examples/named-images.ts](examples/named-images.ts) for building a local
`chromium` image and creating two BOXes from it through the SDK.

## Support

- Secure host path: Linux `x64`, Linux `arm64`
- macOS `arm64`: dev/test only with `SAGENS_ISOLATION_MODE=compat` and `SAGENS_INSECURE_COMPAT=1`
- Node versions: `>=20`
- Linux secure mode requires `/dev/kvm`, delegated cgroup access through `SAGENS_CGROUP_PARENT`, and Linux Landlock support
- The current backend does not support Windows or secure macOS runtime release paths
