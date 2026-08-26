import { describe, it, expect } from 'vitest';
import { classifyBuildOutput, appIdFromLog } from './mobile';

describe('classifyBuildOutput', () => {
  it('returns null while the build is still running', () => {
    expect(classifyBuildOutput('')).toBeNull();
    expect(
      classifyBuildOutput('› Compiling MyApp\nCompiling react-native-reanimated...')
    ).toBeNull();
  });

  it('detects xcodebuild success (expo / react-native)', () => {
    const log = [
      'CompileC build/Objects/Foo.o',
      '** BUILD SUCCEEDED **',
      'Installing on iPhone 17',
    ].join('\n');
    expect(classifyBuildOutput(log)).toBe('launched');
  });

  it('detects flutter success via its interactive banner', () => {
    expect(
      classifyBuildOutput('Syncing files to device iPhone 17...\nFlutter run key commands.')
    ).toBe('launched');
  });

  it('detects a hard xcodebuild failure', () => {
    const log = [
      'CompileC build/Objects/Foo.o',
      "error: use of undeclared identifier 'foo'",
      'The following build commands failed:',
      '** BUILD FAILED **',
    ].join('\n');
    expect(classifyBuildOutput(log)).toBe('failed');
  });

  it('does not false-positive on benign output containing the word "error"', () => {
    // A warning line and a log line that merely contain "error" must not be
    // treated as a build failure — only structural markers count.
    const log =
      'warning: this API is deprecated\nLOG  Handling error boundary gracefully\nBundling complete';
    expect(classifyBuildOutput(log)).toBeNull();
  });

  it('treats failure as authoritative when both markers somehow appear', () => {
    const log = '** BUILD SUCCEEDED **\n...later...\n** BUILD FAILED **';
    expect(classifyBuildOutput(log)).toBe('failed');
  });

  it('does not fire success on a bare "BUILD SUCCEEDED" from an intermediate target', () => {
    // Only xcodebuild's final `** BUILD SUCCEEDED **` banner counts; a pre-build
    // target succeeding must not be read as the app having launched.
    expect(classifyBuildOutput('=== BUILD TARGET Pods ===\nBUILD SUCCEEDED')).toBeNull();
  });

  it('detects a Gradle (Android) build failure', () => {
    // `react-native run-android` / `expo run:android` fail through Gradle, whose
    // marker differs from xcodebuild's. The bare word "android" in a healthy line
    // must not false-positive.
    const log = [
      '> Task :app:compileDebugJavaWithJavac FAILED',
      'FAILURE: Build failed with an exception.',
    ].join('\n');
    expect(classifyBuildOutput(log)).toBe('failed');
    expect(classifyBuildOutput('Building for Android…\nInstalling APK')).toBeNull();
  });
});

describe('appIdFromLog', () => {
  it('parses the Android applicationId from bare-RN run-android (am start cmp=)', () => {
    const log =
      'info Starting the app on "emulator-5554"...\n' +
      'Starting: Intent { act=android.intent.action.MAIN cat=[…] cmp=com.rnpreviewtest/.MainActivity }';
    expect(appIdFromLog('android', log)).toBe('com.rnpreviewtest');
  });

  it('parses the Android applicationId from Expo run:android (dev-client deep link)', () => {
    // Regression: Expo logs `Opening com.x://…` with no `cmp=`, which left the
    // launch poll stuck on "Building…" forever before this branch existed.
    const log =
      '› Opening com.cripcode.testexpoapp://expo-development-client/?url=http%3A%2F%2F192.168.1.2%3A8081';
    expect(appIdFromLog('android', log)).toBe('com.cripcode.testexpoapp');
  });

  it('does not mistake a plain http(s) URL for an app id', () => {
    expect(appIdFromLog('android', 'Opening http://localhost:8081 in the browser')).toBeUndefined();
    expect(appIdFromLog('android', 'no package here')).toBeUndefined();
  });

  it('parses the iOS bundle id from the Expo build log', () => {
    expect(appIdFromLog('ios', '› Opening on iPhone 17 Pro (com.anonymous.my-app)')).toBe(
      'com.anonymous.my-app'
    );
    expect(appIdFromLog('ios', 'nothing to see')).toBeUndefined();
  });
});
