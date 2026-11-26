#import "candidate_panel.h"
#import <QuartzCore/QuartzCore.h>
#include <math.h>
#include <stdio.h>

static const CGFloat RowHeight = 25;
static const CGFloat Padding = 4;
static const CGFloat Gap = 4;

NSRect SweetCandidatePanelFrame(NSUInteger rows, CGFloat preferredWidth, NSRect anchor, NSRect screen) {
    CGFloat width = MIN(NSWidth(screen), MAX(120, MIN(440, preferredWidth)));
    CGFloat height = rows * RowHeight + 2 * Padding;
    CGFloat x = MAX(NSMinX(screen), MIN(NSMinX(anchor), NSMaxX(screen) - width));
    CGFloat y = NSMinY(anchor) - Gap - height;
    if (y < NSMinY(screen)) y = NSMaxY(anchor) + Gap;
    y = MAX(NSMinY(screen), MIN(y, NSMaxY(screen) - height));
    return NSMakeRect(x, y, width, height);
}

static NSScreen *ScreenForAnchor(NSRect anchor) {
    NSPoint point = NSMakePoint(NSMidX(anchor), NSMidY(anchor));
    for (NSScreen *screen in NSScreen.screens) {
        if (NSPointInRect(point, screen.frame)) return screen;
    }
    return nil;
}

static CGFloat LinearColorComponent(CGFloat component) {
    return component <= 0.04045 ? component / 12.92 : pow((component + 0.055) / 1.055, 2.4);
}

static NSColor *SelectionTextColor(NSColor *background) {
    NSColor *rgb = [background colorUsingColorSpace:NSColorSpace.sRGBColorSpace];
    if (!rgb) return NSColor.whiteColor;
    CGFloat luminance = 0.2126 * LinearColorComponent(rgb.redComponent) +
                        0.7152 * LinearColorComponent(rgb.greenComponent) +
                        0.0722 * LinearColorComponent(rgb.blueComponent);
    return luminance > 0.179 ? NSColor.blackColor : NSColor.whiteColor;
}

@interface SweetCandidateRow : NSButton
@property(nonatomic) BOOL highlightedCandidate;
@end

@implementation SweetCandidateRow

- (BOOL)isAccessibilitySelected {
    return self.highlightedCandidate;
}

- (BOOL)acceptsFirstMouse:(NSEvent *)event {
    return YES;
}

- (void)drawRect:(NSRect)dirtyRect {
    NSColor *textColor = NSColor.controlTextColor;
    NSColor *selection = NSColor.selectedControlColor;
#ifndef SWEET_MAVERICKS
    if (@available(macOS 10.14, *)) {
        textColor = NSColor.labelColor;
        selection = NSColor.selectedContentBackgroundColor;
    }
#endif
    if (self.highlightedCandidate) {
        NSColor *background = selection;
        [background setFill];
        [[NSBezierPath bezierPathWithRoundedRect:NSInsetRect(self.bounds, 0, 1)
                                        xRadius:5 yRadius:5] fill];
        textColor = SelectionTextColor(background);
    }
    NSMutableAttributedString *title = [self.attributedTitle mutableCopy];
    [title addAttribute:NSForegroundColorAttributeName value:textColor range:NSMakeRange(0, title.length)];
    CGFloat height = ceil(title.size.height) + 1;
    NSRect text = NSInsetRect(self.bounds, 5, 0);
    text.origin.y = floor(NSMidY(self.bounds) - height / 2);
    text.size.height = height;
    NSStringDrawingOptions options = NSStringDrawingUsesLineFragmentOrigin |
                                     NSStringDrawingTruncatesLastVisibleLine;
#ifndef SWEET_MAVERICKS
    if (@available(macOS 10.11, *)) {
        [title drawWithRect:text options:options context:nil];
    }
    else
#endif
    {
        [title drawWithRect:text options:options];
    }
}

@end

@implementation SweetCandidatePanel {
    void (^_selection)(NSUInteger);
    NSArray<NSString *> *_candidates;
    NSArray<SweetCandidateRow *> *_rows;
    NSFont *_font;
    CGFloat _preferredWidth;
    NSParagraphStyle *_titleStyle;
    NSUInteger _selectedRow;
}

- (instancetype)initWithSelection:(void (^)(NSUInteger))selection {
    self = [super initWithContentRect:NSZeroRect
                           styleMask:NSWindowStyleMaskBorderless | NSWindowStyleMaskNonactivatingPanel
                             backing:NSBackingStoreBuffered defer:YES];
    if (self) {
        _selection = [selection copy];
        _font = [NSFont systemFontOfSize:14];
        NSMutableParagraphStyle *titleStyle = [[NSMutableParagraphStyle alloc] init];
        titleStyle.lineBreakMode = NSLineBreakByTruncatingTail;
        _titleStyle = titleStyle;
        _selectedRow = NSNotFound;
        self.level = NSStatusWindowLevel;
        self.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces |
                                  NSWindowCollectionBehaviorFullScreenAuxiliary |
                                  NSWindowCollectionBehaviorTransient;
        self.hidesOnDeactivate = NO;
        self.releasedWhenClosed = NO;
        self.opaque = NO;
        self.backgroundColor = NSColor.clearColor;
        self.hasShadow = YES;
        NSView *background;
#ifndef SWEET_MAVERICKS
        if (@available(macOS 10.14, *)) {
            NSVisualEffectView *effect = [[NSVisualEffectView alloc] initWithFrame:NSZeroRect];
            effect.material = NSVisualEffectMaterialMenu;
            effect.blendingMode = NSVisualEffectBlendingModeBehindWindow;
            effect.state = NSVisualEffectStateActive;
            effect.wantsLayer = YES;
            background = effect;
        }
        else
#endif
        {
            background = [[NSView alloc] initWithFrame:NSZeroRect];
            background.wantsLayer = YES;
            background.layer.backgroundColor = NSColor.windowBackgroundColor.CGColor;
        }
        background.layer.cornerRadius = 8;
        background.layer.masksToBounds = YES;
#ifndef SWEET_MAVERICKS
        if (@available(macOS 10.10, *)) {
            [background setAccessibilityElement:YES];
            [background setAccessibilityRole:NSAccessibilityListRole];
            [background setAccessibilityLabel:@"変換候補"];
        }
#endif
        self.contentView = background;
    }
    return self;
}

- (BOOL)canBecomeKeyWindow {
    return NO;
}

- (BOOL)canBecomeMainWindow {
    return NO;
}

- (void)showCandidates:(NSArray<NSString *> *)candidates selectedRow:(NSUInteger)selected anchor:(NSRect)anchor {
    NSScreen *screen = ScreenForAnchor(anchor);
    if (!screen || !candidates.count) {
        [self hide];
        return;
    }
    BOOL changed = ![_candidates isEqualToArray:candidates];
    if (changed) {
        for (NSView *row in _rows) [row removeFromSuperview];
        _candidates = [candidates copy];
        NSMutableArray<SweetCandidateRow *> *rows = [NSMutableArray arrayWithCapacity:candidates.count];
        _preferredWidth = 0;
        for (NSUInteger i = 0; i < candidates.count; i++) {
            SweetCandidateRow *row = [[SweetCandidateRow alloc] initWithFrame:NSZeroRect];
            NSString *title = [NSString stringWithFormat:@"%lu.  %@", (unsigned long)i + 1, candidates[i]];
            row.attributedTitle = [[NSAttributedString alloc] initWithString:title
                attributes:@{NSFontAttributeName: _font, NSParagraphStyleAttributeName: _titleStyle}];
            row.bordered = NO;
            row.tag = (NSInteger)i;
            row.target = self;
            row.action = @selector(choose:);
#ifndef SWEET_MAVERICKS
            if (@available(macOS 10.10, *)) [row setAccessibilityLabel:candidates[i]];
#endif
            _preferredWidth = MAX(_preferredWidth, ceil(row.attributedTitle.size.width) + 20);
            [self.contentView addSubview:row];
            [rows addObject:row];
        }
        _rows = rows;
#ifndef SWEET_MAVERICKS
        if (@available(macOS 10.10, *)) [self.contentView setAccessibilityChildren:_rows];
#endif
    }
    NSRect frame = SweetCandidatePanelFrame(candidates.count, _preferredWidth, anchor, screen.visibleFrame);
    [self setFrame:frame display:NO];
    for (NSUInteger i = 0; i < _rows.count; i++) {
        SweetCandidateRow *row = _rows[i];
        row.frame = NSMakeRect(Padding, NSHeight(frame) - Padding - (i + 1) * RowHeight,
                               NSWidth(frame) - 2 * Padding, RowHeight);
        BOOL highlight = i == selected;
        if (changed || row.highlightedCandidate != highlight) {
            row.highlightedCandidate = highlight;
            [row setNeedsDisplay:YES];
        }
    }
    if (!self.visible) [self orderFrontRegardless];
    if (changed || _selectedRow != selected) {
        _selectedRow = selected;
#ifndef SWEET_MAVERICKS
        if (@available(macOS 10.10, *)) {
            [self.contentView setAccessibilitySelectedChildren:selected < _rows.count ? @[_rows[selected]] : @[]];
            NSAccessibilityPostNotification(self.contentView, NSAccessibilitySelectedChildrenChangedNotification);
        }
#endif
    }
}

- (void)choose:(SweetCandidateRow *)sender {
    if (self.visible && _selection) _selection((NSUInteger)sender.tag);
}

- (void)hide {
    [self orderOut:nil];
}

@end

BOOL SweetCandidatePanelPreview(NSString *path) {
    [NSApplication sharedApplication];
    NSScreen *screen = NSScreen.mainScreen;
    if (!screen) {
        fputs("A screen is required for the candidate panel preview.\n", stderr);
        return NO;
    }
    __block NSUInteger clicked = NSNotFound;
    SweetCandidatePanel *panel = [[SweetCandidatePanel alloc] initWithSelection:^(NSUInteger row) {
        clicked = row;
    }];
    NSRect screenFrame = screen.visibleFrame;
    NSRect anchor = NSMakeRect(NSMidX(screenFrame), NSMidY(screenFrame), 1, 20);
    NSArray<NSString *> *candidates = @[@"仮名", @"かな", @"カナ", @"仮名文字", @"かな文字"];
    [panel showCandidates:candidates selectedRow:0 anchor:anchor];
    if (!panel.visible || panel.keyWindow) return NO;
    CGFloat initialHeight = NSHeight(panel.frame);
    [panel showCandidates:candidates selectedRow:1 anchor:anchor];
    NSMutableArray<SweetCandidateRow *> *rows = [NSMutableArray array];
    for (NSView *view in panel.contentView.subviews) {
        if ([view isKindOfClass:SweetCandidateRow.class]) [rows addObject:(SweetCandidateRow *)view];
    }
    if (rows.count != candidates.count) return NO;
    SweetCandidateRow *first = rows[0];
    SweetCandidateRow *second = rows[1];
    if (first.highlightedCandidate || !second.highlightedCandidate) return NO;
#ifndef SWEET_MAVERICKS
    if (@available(macOS 10.10, *)) {
        if (first.isAccessibilitySelected || !second.isAccessibilitySelected ||
            ![[panel.contentView accessibilitySelectedChildren] isEqual:@[second]]) return NO;
    }
#endif
    [second performClick:nil];
    if (clicked != 1) return NO;
    [panel showCandidates:@[@"仮名"] selectedRow:0 anchor:anchor];
    if (NSHeight(panel.frame) >= initialHeight) return NO;
    [panel showCandidates:candidates selectedRow:1 anchor:anchor];

    NSView *view = panel.contentView;
    [view displayIfNeeded];
    NSBitmapImageRep *bitmap = [view bitmapImageRepForCachingDisplayInRect:view.bounds];
    if (!bitmap) return NO;
    [view cacheDisplayInRect:view.bounds toBitmapImageRep:bitmap];
    NSData *png = [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
    [panel hide];
    return [png writeToFile:path atomically:YES];
}
