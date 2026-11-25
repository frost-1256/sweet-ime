#import <AppKit/AppKit.h>

NSRect SweetCandidatePanelFrame(NSUInteger rows, CGFloat preferredWidth, NSRect anchor, NSRect screen);
BOOL SweetCandidatePanelPreview(NSString *path);

@interface SweetCandidatePanel : NSPanel

- (instancetype)initWithSelection:(void (^)(NSUInteger row))selection;
- (void)showCandidates:(NSArray<NSString *> *)candidates selectedRow:(NSUInteger)selected anchor:(NSRect)anchor;
- (void)hide;

@end
