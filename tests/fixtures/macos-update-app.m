// Disposable application for exercising the updater's real LaunchServices path.
// No keyboard hook, screen capture, or permission requests.
#import <AppKit/AppKit.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

int main(int argc, const char **argv) {
    @autoreleasepool {
        NSApplication *app = NSApplication.sharedApplication;
        [app setActivationPolicy:NSApplicationActivationPolicyProhibited];
        [app finishLaunching];
        NSArray *args = NSProcessInfo.processInfo.arguments;
        NSString *report = args.count > 1 ? args[1] : nil;
        const char *ready = getenv("KEYSTEER_UPDATE_READY_SOCKET");
        NSDictionary *details = @{@"arguments": args, @"ready": ready ? @(ready) : @"",
            @"bundle": NSBundle.mainBundle.bundlePath, @"version": [NSBundle.mainBundle objectForInfoDictionaryKey:@"CFBundleVersion"],
            @"pid": @(getpid()), @"parent": @(getppid())};
        [details writeToFile:report atomically:YES];
        if ([args containsObject:@"--exit"]) return 0;
        if (ready) {
            struct sockaddr_un address = {0};
            address.sun_family = AF_UNIX;
            strlcpy(address.sun_path, ready, sizeof(address.sun_path));
            int fd = socket(AF_UNIX, SOCK_STREAM, 0);
            if (connect(fd, (struct sockaddr *)&address, sizeof(address)) != 0) return 2;
            const char *token = [args containsObject:@"--bad-ready"] ? "X" : "R";
            write(fd, token, 1);
            close(fd);
        }
        // Independent safety deadline if the test itself fails before cleanup.
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 30 * NSEC_PER_SEC), dispatch_get_main_queue(), ^{ exit(0); });
        [app run];
    }
    return 0;
}
