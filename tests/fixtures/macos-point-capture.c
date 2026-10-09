// Windows/Linux: clang -std=c11 -Wall -Wextra -Werror tests/fixtures/macos-point-capture.c -o point-capture-test
// macOS: add -framework CoreGraphics to also exercise real image cropping.
#include "../../src/platform/macos/point_capture.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>

static void check_point(double x, double y, double left, double top,
    double width, double height, double scale) {
    NmkPointCapture tile;
    assert(NmkPlanPointCapture(x, y, left, top, width, height, scale, &tile));
    assert(tile.width <= 32 && tile.height <= 32);
    assert(tile.pixel_x < tile.width && tile.pixel_y < tile.height);
    assert(fabs(tile.x * scale + tile.pixel_x - floor((x - left) * scale)) < 1e-6);
    assert(fabs(tile.y * scale + tile.pixel_y - floor((y - top) * scale)) < 1e-6);
    assert(fabs(tile.source_width * scale - tile.width) < 1e-6);
    assert(fabs(tile.source_height * scale - tile.height) < 1e-6);
    assert(tile.x >= 0 && tile.y >= 0);
    assert(tile.x + tile.source_width <= width + 1e-6);
    assert(tile.y + tile.source_height <= height + 1e-6);
}

static void geometry(void) {
    const double scales[] = {1, 1.25, 1.5, 2, 3};
    const double origins[][2] = {{0, 0}, {-1920, 120}, {1440, -900}};
    for (size_t i = 0; i < sizeof(scales) / sizeof(scales[0]); ++i) {
        for (size_t j = 0; j < sizeof(origins) / sizeof(origins[0]); ++j) {
            double left = origins[j][0], top = origins[j][1];
            check_point(left, top, left, top, 1920, 1080, scales[i]);
            check_point(left + 1919.999, top + 1079.999, left, top, 1920, 1080, scales[i]);
            for (int n = 0; n < 200; ++n) {
                check_point(left + n * 9.5 + 0.125, top + n * 5.25 + 0.375,
                    left, top, 1920, 1080, scales[i]);
            }
        }
    }
    check_point(0.25, 0.25, 0, 0, 1, 1, 2);
    NmkPointCapture tile;
    assert(!NmkPlanPointCapture(NAN, 0, 0, 0, 100, 100, 2, &tile));
    assert(!NmkPlanPointCapture(0, 0, 0, 0, 100, 100, 0, &tile));
    assert(!NmkPlanPointCapture(0, 0, 0, 0, 100, 100, INFINITY, &tile));
    assert(!NmkPlanPointCapture(-0.001, 0, 0, 0, 100, 100, 2, &tile));
    assert(!NmkPlanPointCapture(100, 0, 0, 0, 100, 100, 2, &tile));
    assert(!NmkPlanPointCapture(0, 100, 0, 0, 100, 100, 2, &tile));
    assert(!NmkPlanPointCapture(0, 0, 0, 0, 0, 100, 2, &tile));
    assert(!NmkPlanPointCapture(0, 0, 0, 0, 100, 100, 2, NULL));
}

#ifdef __APPLE__
static void fixed_pixel(void) {
    NmkPointCapture tile;
    // Fractional Retina coordinates and a negative secondary display origin.
    assert(NmkPlanPointCapture(-1723.625, 237.25, -1920, 120, 1920, 1080, 2, &tile));
    uint8_t data[32 * 32 * 4], rgba[4] = {0};
    CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGContextRef context = CGBitmapContextCreate(rgba, 1, 1, 8, 4, space,
        kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big);
    assert(space != NULL && context != NULL);
    for (int frame = 0; frame < 4; ++frame) {
        // Every surrounding pixel changes; only the selected pixel stays green.
        for (size_t i = 0; i < sizeof(data); i += 4) {
            data[i] = frame % 2 ? 255 : 0;
            data[i + 1] = 0;
            data[i + 2] = frame % 2 ? 0 : 255;
            data[i + 3] = 255;
        }
        size_t selected = (tile.pixel_y * tile.width + tile.pixel_x) * 4;
        data[selected] = 0; data[selected + 1] = 255; data[selected + 2] = 0;
        CGDataProviderRef provider = CGDataProviderCreateWithData(NULL, data, sizeof(data), NULL);
        CGImageRef image = CGImageCreate(tile.width, tile.height, 8, 32, tile.width * 4,
            space, kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big,
            provider, NULL, false, kCGRenderingIntentDefault);
        assert(NmkReadPointPixel(image, context, tile));
        assert(rgba[0] == 0 && rgba[1] == 255 && rgba[2] == 0 && rgba[3] == 255);
        NmkPointCapture mismatched = tile;
        mismatched.width++;
        assert(!NmkReadPointPixel(image, context, mismatched));
        mismatched = tile; mismatched.pixel_y = mismatched.height;
        assert(!NmkReadPointPixel(image, context, mismatched));
        CGImageRelease(image);
        CGDataProviderRelease(provider);
    }
    CGContextRelease(context);
    CGColorSpaceRelease(space);
}
#endif

int main(void) {
    geometry();
#ifdef __APPLE__
    fixed_pixel();
    puts("point capture geometry and fixed-pixel image tests passed");
#else
    puts("point capture geometry tests passed (CoreGraphics tests require macOS)");
#endif
    return 0;
}
