# End to end checks

The integration test scripts clone their Bazel workspaces into the ignored
`e2e/` directory and generate Gradle views under `e2e/gv_*`.

```sh
bin/gv_run_tests.sh
bin/gv_run_tests.sh --verify cmp_android_desktop
```

The `cmp_android_desktop` case uses
[`muandrew/playground-bazel`](https://github.com/muandrew/playground-bazel),
specifically its standalone `example/cmp` workspace. It generates one view for
`//android:android_app` and `//desktop:desktop_app`, then (with `--verify`)
runs Gradle's `assembleDebug` task for Android and `build` for desktop. This
checks the APK and desktop distribution ZIP are produced; it does not launch
either app. This workspace requires Bazelisk, Java 17 or later, and Python 3.
Android builds also need an Android SDK configured for Gradle.

To prepare fixture checkouts without running tests, use:

```sh
bin/gv_setup_tests.sh
```
