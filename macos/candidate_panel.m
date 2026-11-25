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

@interface SweetCandidateRow : NSButton
@property(nonatomic) BOOL highlightedCandidate;
@end

@implementation SweetCandidateRow

- (BOOL)acceptsFirstMouse:(NSEvent *)event {
    return YES;
}

- (void)drawRect:(NSRect)dirtyRect {
    if (self.highlightedCandidate) {
        [NSColor.selectedContentBackgroundColor setFill];
        [[NSBezierPath bezierPathWithRoundedRect:NSInsetRect(self.bounds, 0, 1)
                                        xRadius:5 yRadius:5] fill];
    }
    [super drawRect:dirtyRect];
}

@end

@implementation SweetCandidatePanel {
    void (^_selection)(NSUInteger);
    NSArray<NSString *> *_candidates;
    NSArray<SweetCandidateRow *> *_rows;
    NSFont *_font;
    CGFloat _preferredWidth;
    NSParagraphStyle *_titleStyle;
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
        self.level = NSStatusWindowLevel;
        self.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces |
                                  NSWindowCollectionBehaviorFullScreenAuxiliary |
                                  NSWindowCollectionBehaviorTransient;
        self.hidesOnDeactivate = NO;
        self.releasedWhenClosed = NO;
        self.opaque = NO;
        self.backgroundColor = NSColor.clearColor;
        self.hasShadow = YES;
        NSVisualEffectView *background = [[NSVisualEffectView alloc] initWithFrame:NSZeroRect];
        background.material = NSVisualEffectMaterialMenu;
        background.blendingMode = NSVisualEffectBlendingModeBehindWindow;
        background.state = NSVisualEffectStateActive;
        background.wantsLayer = YES;
        background.layer.cornerRadius = 8;
        background.layer.masksToBounds = YES;
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
            row.title = [NSString stringWithFormat:@"%lu.  %@", (unsigned long)i + 1, candidates[i]];
            row.bordered = NO;
            row.alignment = NSTextAlignmentLeft;
            row.cell.lineBreakMode = NSLineBreakByTruncatingTail;
            row.font = _font;
            row.tag = (NSInteger)i;
            row.target = self;
            row.action = @selector(choose:);
            [row setAccessibilityLabel:candidates[i]];
            _preferredWidth = MAX(_preferredWidth, ceil([row.title sizeWithAttributes:@{NSFontAttributeName: _font}].width) + 20);
            [self.contentView addSubview:row];
            [rows addObject:row];
        }
        _rows = rows;
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
            row.attributedTitle = [[NSAttributedString alloc] initWithString:row.title
                attributes:@{NSFontAttributeName: _font, NSParagraphStyleAttributeName: _titleStyle,
                    NSForegroundColorAttributeName:
                    highlight ? NSColor.alternateSelectedControlTextColor : NSColor.labelColor}];
            [row setNeedsDisplay:YES];
        }
    }
    if (!self.visible) [self orderFrontRegardless];
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
    SweetCandidatePanel *panel = [[SweetCandidatePanel alloc] initWithSelection:nil];
    NSRect screenFrame = screen.visibleFrame;
    NSRect anchor = NSMakeRect(NSMidX(screenFrame), NSMidY(screenFrame), 1, 20);
    NSArray<NSString *> *candidates = @[@"仮名", @"かな", @"カナ", @"仮名文字", @"かな文字"];
    [panel showCandidates:candidates selectedRow:0 anchor:anchor];
    CGFloat initialHeight = NSHeight(panel.frame);
    [panel showCandidates:candidates selectedRow:1 anchor:anchor];
    NSArray<__kindof NSView *> *rows = panel.contentView.subviews;
    if (rows.count != candidates.count) return NO;
    SweetCandidateRow *first = rows[0];
    SweetCandidateRow *second = rows[1];
    if (first.highlightedCandidate || !second.highlightedCandidate) return NO;
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
