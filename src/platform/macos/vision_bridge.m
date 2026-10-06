#import <CoreGraphics/CoreGraphics.h>
#import <Foundation/Foundation.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <Vision/Vision.h>

#include <stdlib.h>
#include <stdatomic.h>
#include <string.h>
#include <math.h>
#include <unistd.h>

typedef struct {
    bool detect_text;
    bool detect_rectangles;
    bool detect_contours;
    uint64_t timeout_ms;
    double minimum_confidence;
    uint64_t rectangle_max_candidates;
    double rectangle_min_size;
    double rectangle_min_aspect;
    double rectangle_max_aspect;
    uint64_t maximum_capture_pixels;
    uint64_t maximum_capture_dimension;
} NmkVisionConfig;

typedef struct {
    double x;
    double y;
    double width;
    double height;
    double confidence;
    bool is_text;
    char *label;
    uint64_t label_len;
} NmkVisionRegion;

typedef struct {
    uint32_t abi_version;
    uint32_t result_size;
    uint32_t region_stride;
    int32_t status;
    NmkVisionRegion *regions;
    uint64_t count;
    char *message;
    uint64_t message_len;
    CGRect captured_bounds;
    CGImageRef image;
    uint8_t *gray;
    uint64_t gray_width;
    uint64_t gray_height;
} NmkVisionResult;

enum {
    NMK_VISION_OK = 0,
    NMK_VISION_PERMISSION = 1,
    NMK_VISION_TIMEOUT = 2,
    NMK_VISION_FAILED = 3,
    NMK_VISION_CONTEXT_CHANGED = 4,
};

enum { NMK_VISION_ABI_VERSION = 3, NMK_MAX_VISION_REGIONS = 10000 };

static _Atomic uint64_t latestVisionScan = 0;
static _Atomic bool captureInFlight = false;

void NmkSetLatestVisionScan(uint64_t scanID) {
    atomic_store_explicit(&latestVisionScan, scanID, memory_order_release);
}

static bool scanIsCurrent(uint64_t scanID) {
    return atomic_load_explicit(&latestVisionScan, memory_order_acquire) == scanID;
}

static bool tryAcquireCapture(void) {
    bool expected = false;
    return atomic_compare_exchange_strong_explicit(
        &captureInFlight,
        &expected,
        true,
        memory_order_acq_rel,
        memory_order_acquire);
}

static void releaseCapture(void) {
    atomic_store_explicit(&captureInFlight, false, memory_order_release);
}

static NmkVisionResult *resultWithStatus(int32_t status, NSString *message) {
    NmkVisionResult *result = calloc(1, sizeof(NmkVisionResult));
    if (result == NULL) return NULL;
    result->abi_version = NMK_VISION_ABI_VERSION;
    result->result_size = sizeof(NmkVisionResult);
    result->region_stride = sizeof(NmkVisionRegion);
    result->status = status;
    if (message.length > 0) {
        const char *utf8 = message.UTF8String;
        if (utf8 != NULL) {
            result->message_len = (uint64_t)strlen(utf8);
            result->message = strdup(utf8);
        }
    }
    return result;
}

static SCDisplay *displayForID(CGDirectDisplayID displayID, SCShareableContent *content) {
    for (SCDisplay *display in content.displays) {
        if (display.displayID == displayID) return display;
    }
    return content.displays.firstObject;
}

static CGImageRef captureRegion(
    CGRect region,
    uint64_t timeoutMS,
    uint64_t maximumPixels,
    uint64_t maximumDimension,
    int32_t *status,
    NSString **message,
    CGRect *capturedBounds) {
    CGDirectDisplayID ids[32];
    uint32_t count = 0;
    if (CGGetDisplaysWithRect(region, 32, ids, &count) != kCGErrorSuccess || count == 0) {
        *status = NMK_VISION_FAILED;
        *message = @"No display contains the focused window";
        return NULL;
    }
    CGDirectDisplayID displayID = ids[0];
    CGRect displayBounds = CGDisplayBounds(displayID);
    CGRect clipped = CGRectIntersection(region, displayBounds);
    if (CGRectIsNull(clipped) || CGRectIsEmpty(clipped)) {
        *status = NMK_VISION_FAILED;
        *message = @"The focused window is outside the captured display";
        return NULL;
    }
    if (capturedBounds != NULL) *capturedBounds = clipped;

    // ScreenCaptureKit has no cancellation API. Keep this permit until the
    // actual completion callback, not merely until the synchronous caller
    // times out, so full-resolution captures can never overlap.
    if (!tryAcquireCapture()) {
        *status = NMK_VISION_TIMEOUT;
        *message = @"A previous screen capture is still completing";
        return NULL;
    }

    dispatch_group_t group = dispatch_group_create();
    __block CGImageRef captured = NULL;
    __block NSError *captureError = nil;
    dispatch_group_enter(group);
    [SCShareableContent getShareableContentWithCompletionHandler:^(SCShareableContent *content, NSError *error) {
        if (error != nil || content.displays.count == 0) {
            captureError = error;
            releaseCapture();
            dispatch_group_leave(group);
            return;
        }
        SCDisplay *display = displayForID(displayID, content);
        if (display == nil) {
            releaseCapture();
            dispatch_group_leave(group);
            return;
        }
        NSMutableArray<SCWindow *> *excludedWindows = [NSMutableArray array];
        pid_t ownPID = getpid();
        for (SCWindow *window in content.windows) {
            if (window.owningApplication.processID == ownPID) {
                [excludedWindows addObject:window];
            }
        }
        SCContentFilter *filter = [[SCContentFilter alloc]
            initWithDisplay:display excludingWindows:excludedWindows];
        SCStreamConfiguration *configuration = [[SCStreamConfiguration alloc] init];
        CGFloat scaleX = (CGFloat)CGDisplayPixelsWide(displayID) / displayBounds.size.width;
        CGFloat scaleY = (CGFloat)CGDisplayPixelsHigh(displayID) / displayBounds.size.height;
        configuration.sourceRect = CGRectMake(
            clipped.origin.x - displayBounds.origin.x,
            clipped.origin.y - displayBounds.origin.y,
            clipped.size.width,
            clipped.size.height);
        double captureWidth = MAX(1.0, clipped.size.width * scaleX);
        double captureHeight = MAX(1.0, clipped.size.height * scaleY);
        double scale = 1.0;
        if (maximumDimension > 0) {
            scale = MIN(scale, (double)maximumDimension / captureWidth);
            scale = MIN(scale, (double)maximumDimension / captureHeight);
        }
        if (maximumPixels > 0 && captureWidth * captureHeight > (double)maximumPixels) {
            scale = MIN(scale, sqrt((double)maximumPixels / (captureWidth * captureHeight)));
        }
        scale = MIN(1.0, scale);
        configuration.width = MAX(1, (size_t)llround(captureWidth * scale));
        configuration.height = MAX(1, (size_t)llround(captureHeight * scale));
        configuration.showsCursor = NO;
        [SCScreenshotManager captureImageWithFilter:filter configuration:configuration completionHandler:^(CGImageRef image, NSError *error) {
            captureError = error;
            if (image != NULL) captured = CGImageRetain(image);
            releaseCapture();
            dispatch_group_leave(group);
        }];
    }];

    uint64_t boundedTimeoutMS = MIN(MAX(timeoutMS, 1), 30000);
    dispatch_time_t deadline = dispatch_time(
        DISPATCH_TIME_NOW,
        (int64_t)boundedTimeoutMS * NSEC_PER_MSEC);
    if (dispatch_group_wait(group, deadline) != 0) {
        // The ScreenCaptureKit operation cannot be cancelled. Release a late
        // image after its completion block leaves the group instead of leaking
        // the retained CGImage when the caller has already timed out.
        dispatch_group_notify(group, dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
            if (captured != NULL) CGImageRelease(captured);
        });
        *status = NMK_VISION_TIMEOUT;
        *message = @"Screen capture timed out";
        return NULL;
    }
    if (captured == NULL) {
        *status = NMK_VISION_FAILED;
        *message = captureError.localizedDescription ?: @"ScreenCaptureKit did not return an image";
    }
    return captured;
}

void NmkFreeVisionResult(NmkVisionResult *result);

// A point sampler retains only a display filter, a tiny configuration and one
// sRGB pixel context. Capture callbacks own their state across bounded waits.
@interface NmkPointSampler : NSObject {
@public
    uint8_t pixel[4];
    CGContextRef context;
}
@property(nonatomic, strong) SCContentFilter *filter;
@property(nonatomic) CGDirectDisplayID displayID;
@end

@implementation NmkPointSampler
- (instancetype)init {
    self = [super init];
    if (self) {
        CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
        context = CGBitmapContextCreate(pixel, 1, 1, 8, 4, space,
            kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big);
        CGColorSpaceRelease(space);
        if (context == NULL) return nil;
    }
    return self;
}
- (void)dealloc { if (context != NULL) CGContextRelease(context); }
@end

bool NmkSamplePoint(void **owner, double x, double y, bool reset, uint8_t *rgba) {
    @autoreleasepool {
        if (reset) {
            if (*owner != NULL) { id old = CFBridgingRelease(*owner); *owner = NULL; (void)old; }
            return false;
        }
        if (!isfinite(x) || !isfinite(y) || !CGPreflightScreenCaptureAccess()) return false;
        CGDirectDisplayID displayID = 0;
        uint32_t count = 0;
        if (CGGetDisplaysWithPoint(CGPointMake(x, y), 1, &displayID, &count) != kCGErrorSuccess || count == 0) return false;
        if (*owner == NULL) *owner = (void *)CFBridgingRetain([[NmkPointSampler alloc] init]);
        NmkPointSampler *state = (__bridge NmkPointSampler *)*owner;
        if (state == nil || !tryAcquireCapture()) return false;
        CGRect bounds = CGDisplayBounds(displayID);
        CGFloat scaleX = (CGFloat)CGDisplayPixelsWide(displayID) / bounds.size.width;
        CGFloat scaleY = (CGFloat)CGDisplayPixelsHigh(displayID) / bounds.size.height;
        SCStreamConfiguration *configuration = [[SCStreamConfiguration alloc] init];
        configuration.sourceRect = CGRectMake(floor((x - bounds.origin.x) * scaleX) / scaleX,
            floor((y - bounds.origin.y) * scaleY) / scaleY, 1.0 / scaleX, 1.0 / scaleY);
        configuration.width = 1;
        configuration.height = 1;
        configuration.showsCursor = NO;
        dispatch_group_t group = dispatch_group_create();
        dispatch_group_enter(group);
        __block CGImageRef captured = NULL;
        void (^capture)(SCContentFilter *) = ^(SCContentFilter *filter) {
            if (filter == nil) { releaseCapture(); dispatch_group_leave(group); return; }
            [SCScreenshotManager captureImageWithFilter:filter configuration:configuration completionHandler:^(CGImageRef image, NSError *error) {
                if (error == nil && image != NULL) captured = CGImageRetain(image);
                releaseCapture();
                dispatch_group_leave(group);
            }];
        };
        SCContentFilter *cached = nil;
        @synchronized(state) { if (state.displayID == displayID) cached = state.filter; }
        if (cached != nil) capture(cached);
        else [SCShareableContent getShareableContentWithCompletionHandler:^(SCShareableContent *content, NSError *error) {
            SCDisplay *display = nil;
            for (SCDisplay *item in content.displays) if (item.displayID == displayID) { display = item; break; }
            if (error != nil || display == nil) { capture(nil); return; }
            NSMutableArray<SCRunningApplication *> *excluded = [NSMutableArray array];
            for (SCRunningApplication *app in content.applications) if (app.processID == getpid()) [excluded addObject:app];
            SCContentFilter *filter = [[SCContentFilter alloc] initWithDisplay:display excludingApplications:excluded exceptingWindows:@[]];
            @synchronized(state) { state.displayID = displayID; state.filter = filter; }
            capture(filter);
        }];
        if (dispatch_group_wait(group, dispatch_time(DISPATCH_TIME_NOW, 200 * NSEC_PER_MSEC)) != 0) {
            dispatch_group_notify(group, dispatch_get_global_queue(QOS_CLASS_UTILITY, 0), ^{
                if (captured != NULL) CGImageRelease(captured);
            });
            return false;
        }
        if (captured == NULL) return false;
        CGContextSetBlendMode(state->context, kCGBlendModeCopy);
        CGContextDrawImage(state->context, CGRectMake(0, 0, 1, 1), captured);
        CGImageRelease(captured);
        memcpy(rgba, state->pixel, 4);
        return true;
    }
}

NmkVisionResult *NmkDetectVisionElements(
    CGRect windowBounds,
    NmkVisionConfig config,
    uint64_t scanID) {
    @autoreleasepool {
        config.timeout_ms = MIN(MAX(config.timeout_ms, 1), 30000);
        config.rectangle_max_candidates = MIN(
            MAX(config.rectangle_max_candidates, 1),
            (uint64_t)NMK_MAX_VISION_REGIONS);
        if (!scanIsCurrent(scanID)) {
            return resultWithStatus(NMK_VISION_CONTEXT_CHANGED, nil);
        }
        if (!CGPreflightScreenCaptureAccess()) {
            // Scanning is an automatic worker operation, not an explicit
            // permission UI. Requesting access here can contend with System
            // Settings while the user is removing a TCC grant. Only preflight
            // on the worker and let the normal permission guidance handle it.
            return resultWithStatus(NMK_VISION_PERMISSION, @"Screen Recording permission is required for Vision hints");
        }

        int32_t captureStatus = NMK_VISION_OK;
        NSString *captureMessage = nil;
        CGRect capturedBounds = CGRectZero;
        CGImageRef image = captureRegion(
            windowBounds,
            config.timeout_ms,
            config.maximum_capture_pixels,
            config.maximum_capture_dimension,
            &captureStatus,
            &captureMessage,
            &capturedBounds);
        if (image == NULL) return resultWithStatus(captureStatus, captureMessage);
        if (!scanIsCurrent(scanID)) {
            CGImageRelease(image);
            return resultWithStatus(NMK_VISION_CONTEXT_CHANGED, nil);
        }

        NmkVisionResult *result = resultWithStatus(NMK_VISION_OK, nil);
        if (result == NULL) { CGImageRelease(image); return NULL; }
        result->captured_bounds = capturedBounds;
        result->image = image;
        if (config.detect_contours) {
            size_t width = CGImageGetWidth(image), height = CGImageGetHeight(image);
            double scale = MIN(1.0, MIN(2560.0 / MAX(width, height),
                sqrt(2073600.0 / ((double)width * height))));
            width = MAX(1, (size_t)floor(width * scale));
            height = MAX(1, (size_t)floor(height * scale));
            result->gray = calloc(width, height);
            CGColorSpaceRef space = CGColorSpaceCreateDeviceGray();
            CGContextRef context = result->gray != NULL && space != NULL
                ? CGBitmapContextCreate(result->gray, width, height, 8, width, space, (CGBitmapInfo)kCGImageAlphaNone)
                : NULL;
            if (space != NULL) CGColorSpaceRelease(space);
            if (context == NULL) {
                NmkFreeVisionResult(result);
                return resultWithStatus(NMK_VISION_FAILED, @"Cannot prepare contour luma frame");
            }
            // Bitmap row zero is the top row of the untransformed image.
            CGContextSetInterpolationQuality(context, kCGInterpolationLow);
            CGContextDrawImage(context, CGRectMake(0, 0, width, height), image);
            CGContextRelease(context);
            result->gray_width = width;
            result->gray_height = height;
        }
        if (!scanIsCurrent(scanID)) {
            NmkFreeVisionResult(result);
            return resultWithStatus(NMK_VISION_CONTEXT_CHANGED, nil);
        }
        return result;
    }
}

// The capture result retains imageHandle until this synchronous call returns.
NmkVisionResult *NmkRecognizeVisionImage(
    uintptr_t imageHandle, CGRect capturedBounds, NmkVisionConfig config, uint64_t scanID) {
    @autoreleasepool {
        CGImageRef image = (CGImageRef)imageHandle;
        if (image == NULL || !scanIsCurrent(scanID)) {
            return resultWithStatus(NMK_VISION_CONTEXT_CHANGED, nil);
        }
        config.rectangle_max_candidates = MIN(MAX(config.rectangle_max_candidates, 1),
            (uint64_t)NMK_MAX_VISION_REGIONS);
        NSMutableArray<VNRequest *> *requests = [NSMutableArray array];
        VNDetectRectanglesRequest *rectangleRequest = nil;
        VNRecognizeTextRequest *textRequest = nil;
        if (config.detect_rectangles) {
            rectangleRequest = [[VNDetectRectanglesRequest alloc] init];
            rectangleRequest.maximumObservations = (NSUInteger)config.rectangle_max_candidates;
            rectangleRequest.minimumSize = (float)config.rectangle_min_size;
            rectangleRequest.minimumAspectRatio = (float)config.rectangle_min_aspect;
            rectangleRequest.maximumAspectRatio = (float)config.rectangle_max_aspect;
            [requests addObject:rectangleRequest];
        }
        if (config.detect_text) {
            textRequest = [[VNRecognizeTextRequest alloc] init];
            textRequest.recognitionLevel = VNRequestTextRecognitionLevelFast;
            textRequest.usesLanguageCorrection = NO;
            [requests addObject:textRequest];
        }

        VNImageRequestHandler *handler = [[VNImageRequestHandler alloc] initWithCGImage:image options:@{}];
        NSError *error = nil;
        BOOL performed = [handler performRequests:requests error:&error];
        if (!scanIsCurrent(scanID)) {
            return resultWithStatus(NMK_VISION_CONTEXT_CHANGED, nil);
        }
        if (!performed || error != nil) {
            return resultWithStatus(NMK_VISION_FAILED, error.localizedDescription ?: @"Vision request failed");
        }

        NSArray<VNRectangleObservation *> *rectangleResults = rectangleRequest.results ?: @[];
        NSArray<VNRecognizedTextObservation *> *textResults = textRequest.results ?: @[];
        NSUInteger capacity = MIN(
            (NSUInteger)NMK_MAX_VISION_REGIONS,
            rectangleResults.count + textResults.count);
        NmkVisionResult *result = resultWithStatus(NMK_VISION_OK, nil);
        if (result == NULL) return NULL;
        result->captured_bounds = capturedBounds;
        result->regions = calloc(capacity, sizeof(NmkVisionRegion));
        if (capacity > 0 && result->regions == NULL) {
            free(result);
            return NULL;
        }

        for (VNRecognizedTextObservation *observation in textResults) {
            if (result->count >= capacity) break;
            if (observation.confidence < config.minimum_confidence) continue;
            VNRecognizedText *candidate = [observation topCandidates:1].firstObject;
            CGRect box = observation.boundingBox;
            NmkVisionRegion *out = &result->regions[result->count++];
            out->x = box.origin.x;
            out->y = box.origin.y;
            out->width = box.size.width;
            out->height = box.size.height;
            out->confidence = observation.confidence;
            out->is_text = true;
            NSString *label = candidate.string ?: @"";
            const char *utf8 = label.UTF8String;
            if (utf8 != NULL) {
                out->label_len = (uint64_t)strlen(utf8);
                out->label = strdup(utf8);
            }
        }
        for (VNRectangleObservation *observation in rectangleResults) {
            if (result->count >= capacity) break;
            if (observation.confidence < config.minimum_confidence) continue;
            CGRect box = observation.boundingBox;
            NmkVisionRegion *out = &result->regions[result->count++];
            out->x = box.origin.x;
            out->y = box.origin.y;
            out->width = box.size.width;
            out->height = box.size.height;
            out->confidence = observation.confidence;
            out->is_text = false;
            out->label = strdup("");
            out->label_len = 0;
        }
        if (!scanIsCurrent(scanID)) {
            NmkFreeVisionResult(result);
            return resultWithStatus(NMK_VISION_CONTEXT_CHANGED, nil);
        }
        return result;
    }
}

void NmkFreeVisionResult(NmkVisionResult *result) {
    if (result == NULL) return;
    for (uint64_t index = 0; index < result->count; index++) free(result->regions[index].label);
    if (result->image != NULL) CGImageRelease(result->image);
    free(result->gray);
    free(result->regions);
    free(result->message);
    free(result);
}
