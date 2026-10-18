#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>

static NSString *SafeFontName(NSString *fontName) {
    return [[fontName stringByReplacingOccurrencesOfString:@" " withString:@"_"]
        stringByReplacingOccurrencesOfString:@"/" withString:@"-"];
}

static NSString *RingNumberForIndex(NSInteger index, NSString **shapeName) {
    NSArray<NSString *> *regionCodes = @[
        @"01", @"02", @"03", @"04", @"05", @"06", @"07", @"08", @"09", @"10",
        @"11", @"12", @"13", @"14", @"15", @"16", @"17", @"18", @"19", @"20",
        @"21", @"22", @"23", @"24", @"25", @"26", @"27", @"28", @"29", @"30",
        @"31", @"32", @"33"
    ];
    NSString *region = index % 17 == 0 ? @"JN" : regionCodes[(index * 7) % regionCodes.count];
    NSInteger serialValue = 1000000 + ((index + 1) * 73421) % 9000000;
    NSString *serial = [NSString stringWithFormat:@"%07ld", (long)serialValue];
    NSInteger selector = index % 10;

    if (selector < 4) {
        *shapeName = @"three-part";
        NSInteger year = 2020 + index % 16;
        return [NSString stringWithFormat:@"%ld-%@-%@", (long)year, region, serial];
    }
    if (selector < 7) {
        *shapeName = @"two-part";
        return [NSString stringWithFormat:@"%@-%@", region, serial];
    }

    *shapeName = @"one-part";
    return serial;
}

static BOOL WritePNG(NSImage *image, NSString *path) {
    NSBitmapImageRep *bitmap = [NSBitmapImageRep imageRepWithData:image.TIFFRepresentation];
    NSData *png = [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
    return [png writeToFile:path atomically:YES];
}

static NSRect TextRect(NSString *text, NSDictionary<NSAttributedStringKey, id> *attributes,
                       NSSize canvasSize, NSInteger alignment) {
    NSSize measured = [text sizeWithAttributes:attributes];
    CGFloat x = 0;
    if (alignment == 0) {
        x = 48;
    } else if (alignment == 1) {
        x = (canvasSize.width - measured.width) / 2.0;
    } else {
        x = canvasSize.width - measured.width - 48;
    }
    return NSMakeRect(
        x,
        (canvasSize.height - measured.height) / 2.0,
        ceil(measured.width) + 3,
        ceil(measured.height) + 3
    );
}

int main(void) {
    @autoreleasepool {
        NSArray<NSString *> *fonts = @[
            @"Arial",
            @"Arial Bold",
            @"Arial Narrow",
            @"Helvetica Neue",
            @"Verdana",
            @"Verdana Bold",
            @"Times New Roman",
            @"Times New Roman Bold",
            @"Georgia",
            @"Georgia Bold",
            @"Courier New",
            @"Courier New Bold"
        ];
        NSArray<NSString *> *prefixes = @[
            @"",
            @"RING NO. ",
            @"",
            @"ID: ",
            @"",
            @"BAND ",
            @""
        ];
        NSArray<NSDictionary *> *palettes = @[
            @{
                @"name": @"white",
                @"background": [NSColor whiteColor],
                @"foreground": [NSColor blackColor]
            },
            @{
                @"name": @"light-gray",
                @"background": [NSColor colorWithCalibratedWhite:0.94 alpha:1],
                @"foreground": [NSColor colorWithCalibratedWhite:0.08 alpha:1]
            },
            @{
                @"name": @"warm-white",
                @"background": [NSColor colorWithCalibratedRed:1.0 green:0.98 blue:0.91 alpha:1],
                @"foreground": [NSColor colorWithCalibratedWhite:0.10 alpha:1]
            },
            @{
                @"name": @"pale-blue",
                @"background": [NSColor colorWithCalibratedRed:0.91 green:0.95 blue:0.98 alpha:1],
                @"foreground": [NSColor colorWithCalibratedRed:0.04 green:0.10 blue:0.17 alpha:1]
            },
            @{
                @"name": @"dark",
                @"background": [NSColor colorWithCalibratedWhite:0.10 alpha:1],
                @"foreground": [NSColor colorWithCalibratedWhite:0.96 alpha:1]
            }
        ];

        NSFileManager *fileManager = [NSFileManager defaultManager];
        NSString *projectRoot = fileManager.currentDirectoryPath;
        NSString *outputDirectory = [projectRoot stringByAppendingPathComponent:@"test-images/generated"];
        NSString *contactSheetPath = [projectRoot stringByAppendingPathComponent:@"test-images/generated-contact-sheet.png"];

        if ([fileManager fileExistsAtPath:outputDirectory]) {
            NSError *removeError = nil;
            [fileManager removeItemAtPath:outputDirectory error:&removeError];
            if (removeError != nil) {
                NSLog(@"Unable to replace output directory: %@", removeError);
                return 1;
            }
        }

        NSError *directoryError = nil;
        [fileManager createDirectoryAtPath:outputDirectory
               withIntermediateDirectories:YES
                                attributes:nil
                                     error:&directoryError];
        if (directoryError != nil) {
            NSLog(@"Unable to create output directory: %@", directoryError);
            return 1;
        }

        NSSize canvasSize = NSMakeSize(960, 240);
        NSMutableString *manifest = [NSMutableString stringWithString:
            @"file_name\texpected_number\tshape\tfont\tfont_size\tprefix\tpalette\talignment\n"];
        NSMutableArray<NSString *> *generatedPaths = [NSMutableArray arrayWithCapacity:100];

        for (NSInteger index = 0; index < 100; index++) {
            NSString *shapeName = nil;
            NSString *ringNumber = RingNumberForIndex(index, &shapeName);
            NSString *fontName = fonts[(index * 5) % fonts.count];
            NSInteger pointSize = 20 + (index * 8) % 21;
            if ([fontName isEqualToString:@"Arial"] && pointSize >= 36) {
                fontName = @"Arial Bold";
            }
            if ([fontName isEqualToString:@"Arial Narrow"] && pointSize >= 32) {
                fontName = @"Arial Narrow Bold";
            }
            NSString *prefix = prefixes[(index * 3) % prefixes.count];
            NSDictionary *palette = palettes[(index * 2) % palettes.count];
            NSInteger alignment = index % 3;
            NSString *alignmentName = @[@"left", @"center", @"right"][alignment];
            NSString *displayText = [prefix stringByAppendingString:ringNumber];

            NSFont *font = [NSFont fontWithName:fontName size:pointSize];
            if (font == nil) {
                NSLog(@"Font is unavailable: %@", fontName);
                return 1;
            }

            NSImage *image = [[NSImage alloc] initWithSize:canvasSize];
            [image lockFocus];
            [NSGraphicsContext currentContext].shouldAntialias = YES;
            [palette[@"background"] setFill];
            NSRectFill(NSMakeRect(0, 0, canvasSize.width, canvasSize.height));

            NSDictionary *attributes = @{
                NSFontAttributeName: font,
                NSForegroundColorAttributeName: palette[@"foreground"],
                NSKernAttributeName: @0
            };
            [displayText drawInRect:TextRect(displayText, attributes, canvasSize, alignment)
                     withAttributes:attributes];
            [image unlockFocus];

            NSString *fileName = [NSString stringWithFormat:
                @"%03ld_%@_%@_%ldpt.png",
                (long)(index + 1),
                ringNumber,
                SafeFontName(fontName),
                (long)pointSize
            ];
            NSString *destination = [outputDirectory stringByAppendingPathComponent:fileName];
            if (!WritePNG(image, destination)) {
                NSLog(@"Unable to write %@", destination);
                return 1;
            }
            [generatedPaths addObject:destination];

            [manifest appendFormat:@"%@\t%@\t%@\t%@\t%ld\t%@\t%@\t%@\n",
                fileName,
                ringNumber,
                shapeName,
                fontName,
                (long)pointSize,
                prefix.length == 0 ? @"none" : [prefix stringByTrimmingCharactersInSet:NSCharacterSet.whitespaceCharacterSet],
                palette[@"name"],
                alignmentName
            ];
        }

        NSString *manifestPath = [outputDirectory stringByAppendingPathComponent:@"manifest.tsv"];
        NSError *manifestError = nil;
        [manifest writeToFile:manifestPath
                   atomically:YES
                     encoding:NSUTF8StringEncoding
                        error:&manifestError];
        if (manifestError != nil) {
            NSLog(@"Unable to write manifest: %@", manifestError);
            return 1;
        }

        NSSize sheetSize = NSMakeSize(1920, 2400);
        NSImage *contactSheet = [[NSImage alloc] initWithSize:sheetSize];
        [contactSheet lockFocus];
        [[NSColor colorWithCalibratedWhite:0.78 alpha:1] setFill];
        NSRectFill(NSMakeRect(0, 0, sheetSize.width, sheetSize.height));
        [NSGraphicsContext currentContext].imageInterpolation = NSImageInterpolationHigh;

        CGFloat cellWidth = sheetSize.width / 5.0;
        CGFloat cellHeight = sheetSize.height / 20.0;
        for (NSInteger index = 0; index < generatedPaths.count; index++) {
            NSInteger column = index % 5;
            NSInteger row = index / 5;
            CGFloat x = column * cellWidth + 12;
            CGFloat y = sheetSize.height - (row + 1) * cellHeight + 20;
            NSImage *sample = [[NSImage alloc] initWithContentsOfFile:generatedPaths[index]];
            [sample drawInRect:NSMakeRect(x, y, cellWidth - 24, 90)
                      fromRect:NSZeroRect
                     operation:NSCompositingOperationSourceOver
                      fraction:1.0];
            NSString *indexLabel = [NSString stringWithFormat:@"%03ld", (long)(index + 1)];
            [indexLabel drawAtPoint:NSMakePoint(x, y - 15)
                     withAttributes:@{
                         NSFontAttributeName: [NSFont monospacedSystemFontOfSize:11 weight:NSFontWeightRegular],
                         NSForegroundColorAttributeName: [NSColor blackColor]
                     }];
        }
        [contactSheet unlockFocus];

        if (!WritePNG(contactSheet, contactSheetPath)) {
            NSLog(@"Unable to write contact sheet");
            return 1;
        }

        printf("Generated 100 images in %s\n", outputDirectory.UTF8String);
        printf("Manifest: %s\n", manifestPath.UTF8String);
        printf("Contact sheet: %s\n", contactSheetPath.UTF8String);
    }
    return 0;
}
