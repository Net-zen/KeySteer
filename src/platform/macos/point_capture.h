#ifndef KEYSTEER_POINT_CAPTURE_H
#define KEYSTEER_POINT_CAPTURE_H

#include <math.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

// Capture a small, pixel-aligned tile at 1:1 resolution. Never rely on the
// screenshot API accepting a sub-point sourceRect and a 1x1 output image.
typedef struct {
    double x, y, source_width, source_height;
    size_t width, height, pixel_x, pixel_y;
} NmkPointCapture;

static inline bool NmkPlanPointCapture(
    double x, double y, double left, double top, double width, double height,
    double scale, NmkPointCapture *out) {
    if (out == NULL || !isfinite(x) || !isfinite(y) || !isfinite(left)
        || !isfinite(top) || !isfinite(width) || !isfinite(height)
        || !isfinite(scale) || width <= 0 || height <= 0 || scale <= 0) return false;
    double local_x = x - left, local_y = y - top;
    if (local_x < 0 || local_y < 0 || local_x >= width || local_y >= height) return false;
    double pixels_wide = floor(width * scale), pixels_high = floor(height * scale);
    if (!isfinite(pixels_wide) || !isfinite(pixels_high)
        || pixels_wide < 1 || pixels_high < 1
        || pixels_wide > UINT32_MAX || pixels_high > UINT32_MAX) return false;
    size_t display_width = (size_t)pixels_wide, display_height = (size_t)pixels_high;
    size_t pixel_x = (size_t)fmin(floor(local_x * scale), pixels_wide - 1);
    size_t pixel_y = (size_t)fmin(floor(local_y * scale), pixels_high - 1);
    size_t tile_width = display_width < 32 ? display_width : 32;
    size_t tile_height = display_height < 32 ? display_height : 32;
    size_t tile_x = pixel_x > tile_width / 2 ? pixel_x - tile_width / 2 : 0;
    size_t tile_y = pixel_y > tile_height / 2 ? pixel_y - tile_height / 2 : 0;
    if (tile_x > display_width - tile_width) tile_x = display_width - tile_width;
    if (tile_y > display_height - tile_height) tile_y = display_height - tile_height;
    *out = (NmkPointCapture) {
        .x = tile_x / scale, .y = tile_y / scale,
        .source_width = tile_width / scale, .source_height = tile_height / scale,
        .width = tile_width, .height = tile_height,
        .pixel_x = pixel_x - tile_x, .pixel_y = pixel_y - tile_y,
    };
    return true;
}

#ifdef __APPLE__
#include <CoreGraphics/CoreGraphics.h>

static inline bool NmkReadPointPixel(
    CGImageRef image, CGContextRef context, NmkPointCapture capture) {
    // A differently sized image is not a pixel-accurate sample. In particular,
    // do not shrink a full display (or an unexpectedly scaled tile) into 1x1.
    if (image == NULL || context == NULL
        || CGImageGetWidth(image) != capture.width
        || CGImageGetHeight(image) != capture.height
        || capture.pixel_x >= capture.width || capture.pixel_y >= capture.height) return false;
    CGImageRef pixel = CGImageCreateWithImageInRect(image,
        CGRectMake(capture.pixel_x, capture.pixel_y, 1, 1));
    if (pixel == NULL) return false;
    CGContextSetBlendMode(context, kCGBlendModeCopy);
    CGContextSetInterpolationQuality(context, kCGInterpolationNone);
    CGContextDrawImage(context, CGRectMake(0, 0, 1, 1), pixel);
    CGImageRelease(pixel);
    return true;
}
#endif

#endif
