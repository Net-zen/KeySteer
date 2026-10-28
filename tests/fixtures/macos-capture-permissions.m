// Compile the actual capture bridge with deterministic display/ScreenCaptureKit
// boundaries. This never captures the user's desktop or asks for TCC access.
#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#include <assert.h>

static NSError *contentError;
static NSError *imageError;
static NSUInteger imageRequests;
static bool cancelDuringCapture;
static bool delayCapture;
static void (^pendingCapture)(CGImageRef, NSError *);
static void cancelTestScan(void);

@interface TestDisplay : NSObject
@property(readonly) CGDirectDisplayID displayID;
@end
@implementation TestDisplay
- (CGDirectDisplayID)displayID { return 1; }
@end

@interface TestFilter : NSObject
@property(readonly) CGFloat pointPixelScale;
- (instancetype)initWithDisplay:(SCDisplay *)display
    excludingApplications:(NSArray *)apps exceptingWindows:(NSArray *)windows;
@end
@implementation TestFilter
- (CGFloat)pointPixelScale { return 1; }
- (instancetype)initWithDisplay:(SCDisplay *)display
    excludingApplications:(NSArray *)apps exceptingWindows:(NSArray *)windows { return [super init]; }
@end

@interface TestContent : NSObject
@property(readonly) NSArray<SCDisplay *> *displays;
@property(readonly) NSArray<SCRunningApplication *> *applications;
+ (void)getShareableContentExcludingDesktopWindows:(BOOL)exclude onScreenWindowsOnly:(BOOL)only
    completionHandler:(void (^)(TestContent *, NSError *))completion;
+ (void)getShareableContentWithCompletionHandler:(void (^)(TestContent *, NSError *))completion;
@end
@implementation TestContent
- (NSArray *)displays { return @[[TestDisplay new]]; }
- (NSArray *)applications { return @[]; }
+ (void)getShareableContentExcludingDesktopWindows:(BOOL)exclude onScreenWindowsOnly:(BOOL)only
    completionHandler:(void (^)(TestContent *, NSError *))completion {
    completion(contentError ? nil : [TestContent new], contentError);
}
+ (void)getShareableContentWithCompletionHandler:(void (^)(TestContent *, NSError *))completion {
    [self getShareableContentExcludingDesktopWindows:YES onScreenWindowsOnly:YES completionHandler:completion];
}
@end

@interface TestScreenshot : NSObject
+ (void)captureImageWithFilter:(TestFilter *)filter configuration:(SCStreamConfiguration *)config
    completionHandler:(void (^)(CGImageRef, NSError *))completion;
@end
@implementation TestScreenshot
+ (void)captureImageWithFilter:(TestFilter *)filter configuration:(SCStreamConfiguration *)config
    completionHandler:(void (^)(CGImageRef, NSError *))completion {
    imageRequests++;
    if (cancelDuringCapture) cancelTestScan();
    if (delayCapture) { pendingCapture = [completion copy]; return; }
    CGColorSpaceRef space = CGColorSpaceCreateDeviceRGB();
    CGContextRef bitmap = CGBitmapContextCreate(NULL, 8, 8, 8, 32, space, (CGBitmapInfo)kCGImageAlphaPremultipliedLast);
    CGImageRef image = CGBitmapContextCreateImage(bitmap);
    completion(imageError ? NULL : image, imageError);
    CGImageRelease(image);
    CGContextRelease(bitmap);
    CGColorSpaceRelease(space);
}
@end

static bool testPreflight(void) { return false; }
static CGError testDisplays(CGRect rect, uint32_t max, CGDirectDisplayID *ids, uint32_t *count) {
    ids[0] = 1; *count = 1; return kCGErrorSuccess;
}
static CGRect testBounds(CGDirectDisplayID display) { return CGRectMake(0, 0, 8, 8); }
static CGDisplayModeRef testMode(CGDirectDisplayID display) { return NULL; }

#define CGPreflightScreenCaptureAccess testPreflight
#define CGGetDisplaysWithRect testDisplays
#define CGDisplayBounds testBounds
#define CGDisplayCopyDisplayMode testMode
#define SCContentFilter TestFilter
#define SCShareableContent TestContent
#define SCScreenshotManager TestScreenshot
#include "../../src/platform/macos/vision/vision_bridge.m"

static void cancelTestScan(void) { NmkCancelVisionScan(1); }

static void expectStatus(int32_t status) {
    NmkSetLatestVisionScan(1);
    NmkVisionConfig config = {0};
    config.timeout_ms = delayCapture ? 1 : 100;
    NmkVisionResult *result = NmkDetectVisionElements(CGRectMake(0, 0, 8, 8), config, 1, 0);
    assert(result != NULL && result->status == status);
    if (status == NMK_VISION_PERMISSION) {
        assert(strstr(result->message, "already enabled") != NULL);
    }
    NmkFreeVisionResult(result);
}

int main(void) {
    @autoreleasepool {
        // Regression: a false legacy preflight must not reject successful SCK.
        assert(!testPreflight());
        expectStatus(NMK_VISION_OK);
        assert(imageRequests == 1);

        NSError *denied = [NSError errorWithDomain:SCStreamErrorDomain code:SCStreamErrorUserDeclined userInfo:nil];
        contentError = denied;
        expectStatus(NMK_VISION_PERMISSION);
        assert(imageRequests == 1);
        contentError = nil;
        imageError = denied;
        expectStatus(NMK_VISION_PERMISSION);

        // The same numeric code from a different domain is not a TCC denial.
        imageError = [NSError errorWithDomain:NSCocoaErrorDomain code:SCStreamErrorUserDeclined userInfo:nil];
        expectStatus(NMK_VISION_FAILED);
        imageError = [NSError errorWithDomain:SCStreamErrorDomain code:SCStreamErrorInternalError userInfo:nil];
        expectStatus(NMK_VISION_FAILED);
        imageError = nil;
        expectStatus(NMK_VISION_OK); // A previous denial is never cached.

        cancelDuringCapture = true;
        expectStatus(NMK_VISION_CONTEXT_CHANGED);
        cancelDuringCapture = false;
        delayCapture = true;
        expectStatus(NMK_VISION_TIMEOUT);
        pendingCapture(NULL, nil); // Late callbacks still release the permit.
        pendingCapture = nil;
        delayCapture = false;
        expectStatus(NMK_VISION_OK);
    }
    return 0;
}
