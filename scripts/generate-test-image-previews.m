#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>

static NSRect CenteredRect(NSString *text, NSDictionary<NSAttributedStringKey, id> *attributes,
                           NSSize canvasSize, CGFloat centerY) {
    NSSize measured = [text sizeWithAttributes:attributes];
    return NSMakeRect(
        (canvasSize.width - measured.width) / 2.0,
        centerY - measured.height / 2.0,
        ceil(measured.width) + 2.0,
        ceil(measured.height) + 2.0
    );
}

int main(void) {
    @autoreleasepool {
        NSArray<NSDictionary *> *samples = @[
            @{
                @"fileName": @"001_2026-10-0178948_Arial_20pt.png",
                @"ringNumber": @"2026-10-0178948",
                @"fontName": @"Arial",
                @"pointSize": @20,
                @"background": [NSColor whiteColor],
                @"foreground": [NSColor blackColor]
            },
            @{
                @"fileName": @"002_2026-JN-0387764_TimesNewRomanBold_40pt.png",
                @"ringNumber": @"2026-JN-0387764",
                @"displayText": @"RING NO. 2026-JN-0387764",
                @"fontName": @"Times New Roman Bold",
                @"pointSize": @40,
                @"background": [NSColor colorWithCalibratedWhite:0.94 alpha:1.0],
                @"foreground": [NSColor colorWithCalibratedWhite:0.08 alpha:1.0]
            }
        ];

        NSSize canvasSize = NSMakeSize(960, 240);
        NSString *outputDirectory = [[[NSFileManager defaultManager] currentDirectoryPath]
            stringByAppendingPathComponent:@"test-images/previews"];
        NSError *directoryError = nil;
        [[NSFileManager defaultManager] createDirectoryAtPath:outputDirectory
                                  withIntermediateDirectories:YES
                                                   attributes:nil
                                                        error:&directoryError];
        if (directoryError != nil) {
            NSLog(@"Unable to create output directory: %@", directoryError);
            return 1;
        }

        for (NSDictionary *sample in samples) {
            NSImage *image = [[NSImage alloc] initWithSize:canvasSize];
            [image lockFocus];
            [NSGraphicsContext currentContext].shouldAntialias = YES;

            [sample[@"background"] setFill];
            NSRectFill(NSMakeRect(0, 0, canvasSize.width, canvasSize.height));

            CGFloat pointSize = [sample[@"pointSize"] doubleValue];
            NSFont *numberFont = [NSFont fontWithName:sample[@"fontName"] size:pointSize];
            if (numberFont == nil) {
                numberFont = [NSFont systemFontOfSize:pointSize];
            }
            NSDictionary *numberAttributes = @{
                NSFontAttributeName: numberFont,
                NSForegroundColorAttributeName: sample[@"foreground"],
                NSKernAttributeName: @0
            };

            NSString *ringNumber = sample[@"ringNumber"];
            NSString *displayText = sample[@"displayText"] ?: ringNumber;
            NSString *label = sample[@"label"];
            CGFloat numberCenterY = label == nil ? canvasSize.height / 2.0 : 105.0;
            [displayText drawInRect:CenteredRect(displayText, numberAttributes, canvasSize, numberCenterY)
                    withAttributes:numberAttributes];

            if (label != nil) {
                NSFont *labelFont = [NSFont fontWithName:@"Helvetica Neue" size:14]
                    ?: [NSFont systemFontOfSize:14];
                NSDictionary *labelAttributes = @{
                    NSFontAttributeName: labelFont,
                    NSForegroundColorAttributeName: [NSColor colorWithCalibratedWhite:0.35 alpha:1.0],
                    NSKernAttributeName: @1.2
                };
                [label drawInRect:CenteredRect(label, labelAttributes, canvasSize, 158.0)
                   withAttributes:labelAttributes];
            }

            [image unlockFocus];

            NSBitmapImageRep *bitmap = [NSBitmapImageRep imageRepWithData:image.TIFFRepresentation];
            NSData *png = [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
            NSString *destination = [outputDirectory stringByAppendingPathComponent:sample[@"fileName"]];
            if (![png writeToFile:destination atomically:YES]) {
                NSLog(@"Unable to write %@", destination);
                return 1;
            }
            printf("%s\n", destination.UTF8String);
        }
    }
    return 0;
}
