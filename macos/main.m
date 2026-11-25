#import <AppKit/AppKit.h>
#import <Carbon/Carbon.h>
#import <InputMethodKit/InputMethodKit.h>
#import <dispatch/dispatch.h>

#include <stdlib.h>
#include <string.h>

#include "bridge.h"

static NSRange NoReplacement(void) {
    return NSMakeRange(NSNotFound, 0);
}

static NSDictionary *Snapshot(SweetContext *context) {
    const char *snapshot = sweet_macos_snapshot(context);
    NSData *data = [NSData dataWithBytes:snapshot length:strlen(snapshot)];
    return [NSJSONSerialization JSONObjectWithData:data options:0 error:NULL];
}

// The shared keymap uses Windows virtual keys. Use the keyboard layout's
// unshifted characters for letters and symbols, and Carbon codes for editing keys.
static uint16_t VirtualKey(NSEvent *event) {
    switch (event.keyCode) {
        case kVK_Return: case kVK_ANSI_KeypadEnter: return 0x0D;
        case kVK_Tab: return 0x09;
        case kVK_Space: return 0x20;
        case kVK_Delete: return 0x08;
        case kVK_ForwardDelete: return 0x2E;
        case kVK_Escape: return 0x1B;
        case kVK_LeftArrow: return 0x25;
        case kVK_UpArrow: return 0x26;
        case kVK_RightArrow: return 0x27;
        case kVK_DownArrow: return 0x28;
        case kVK_Home: return 0x24;
        case kVK_End: return 0x23;
        case kVK_PageUp: return 0x21;
        case kVK_PageDown: return 0x22;
        case kVK_F1: return 0x70;
        case kVK_F2: return 0x71;
        case kVK_F3: return 0x72;
        case kVK_F4: return 0x73;
        case kVK_F5: return 0x74;
        case kVK_F6: return 0x75;
        case kVK_F7: return 0x76;
        case kVK_F8: return 0x77;
        case kVK_F9: return 0x78;
        case kVK_F10: return 0x79;
        case kVK_F11: return 0x7A;
        case kVK_F12: return 0x7B;
        case kVK_JIS_Eisu: return 0xF0;
        case kVK_JIS_Kana: return 0xF2;
    }
    NSString *unshifted = [[event charactersByApplyingModifiers:0] uppercaseString];
    if (unshifted.length != 1) return 0;
    unichar ch = [unshifted characterAtIndex:0];
    if ((ch >= 'A' && ch <= 'Z') || (ch >= '0' && ch <= '9')) return ch;
    switch (ch) {
        case ';': return 0xBA;
        case '=': return 0xBB;
        case ',': return 0xBC;
        case '-': return 0xBD;
        case '.': return 0xBE;
        case '/': return 0xBF;
        case '`': return 0xC0;
        case '[': return 0xDB;
        case '\\': return 0xDC;
        case ']': return 0xDD;
        case '\'': return 0xDE;
        default: return 0;
    }
}

@interface SweetInputController : IMKInputController
@end

@implementation SweetInputController {
    SweetContext *_context;
    IMKCandidates *_panel;
    NSArray<NSString *> *_candidates;
    BOOL _marked;
    BOOL _updatingPanel;
    BOOL _pollScheduled;
}

- (instancetype)initWithServer:(IMKServer *)server delegate:(id)delegate client:(id)client {
    self = [super initWithServer:server delegate:delegate client:client];
    if (self) {
        _context = sweet_macos_new();
        _panel = [[IMKCandidates alloc] initWithServer:server panelType:kIMKSingleColumnScrollingCandidatePanel];
    }
    return self;
}

- (void)dealloc {
    [_panel hide];
    if (_context) sweet_macos_free(_context);
}

- (NSUInteger)recognizedEvents:(id)sender {
    return NSEventMaskKeyDown;
}

- (void)activateServer:(id)sender {
    [super activateServer:sender];
    if (_context) sweet_macos_activate(_context);
}

- (void)deactivateServer:(id)sender {
    [self commitComposition:sender];
    [super deactivateServer:sender];
}

- (void)discardComposition {
    sweet_macos_finish(_context, false);
    [_panel hide];
    _candidates = nil;
    _marked = NO;
}

- (void)commitComposition:(id)sender {
    if (!_context) return;
    if (IsSecureEventInputEnabled()) {
        [self discardComposition];
        return;
    }
    sweet_macos_finish(_context, true);
    [self publishToClient:sender];
}

- (BOOL)handleEvent:(NSEvent *)event client:(id)sender {
    if (!_context || event.type != NSEventTypeKeyDown) return NO;
    if (IsSecureEventInputEnabled()) {
        [self discardComposition];
        return NO;
    }
    NSEventModifierFlags flags = event.modifierFlags;
    if (flags & NSEventModifierFlagCommand) return NO;
    uint8_t modifiers = ((flags & NSEventModifierFlagShift) ? 1 : 0)
        | ((flags & NSEventModifierFlagControl) ? 2 : 0)
        | ((flags & NSEventModifierFlagOption) ? 4 : 0);
    NSString *characters = event.characters;
    uint32_t ch = characters.length == 1 ? [characters characterAtIndex:0] : 0;
    if (!sweet_macos_key(_context, VirtualKey(event), ch, modifiers)) return NO;
    [self publishToClient:sender];
    [self scheduleLiveUpdate];
    return YES;
}

- (void)scheduleLiveUpdate {
    if (_pollScheduled || !sweet_macos_live_pending(_context)) return;
    _pollScheduled = YES;
    __weak SweetInputController *controller = self;
    dispatch_after(dispatch_time(DISPATCH_TIME_NOW, 16 * NSEC_PER_MSEC), dispatch_get_main_queue(), ^{
        [controller pollLiveUpdate];
    });
}

- (void)pollLiveUpdate {
    _pollScheduled = NO;
    id client = [self client];
    if (!client || IsSecureEventInputEnabled()) {
        [self discardComposition];
        return;
    }
    if (sweet_macos_apply_live(_context)) [self publishToClient:client];
    [self scheduleLiveUpdate];
}

- (void)publishToClient:(id<IMKTextInput>)client {
    NSDictionary *state = Snapshot(_context);
    NSString *commit = state[@"commit"];
    NSString *preedit = state[@"preedit"];
    if (commit.length) {
        _marked = NO;
        [client insertText:commit replacementRange:NoReplacement()];
    }
    if (preedit.length) {
        NSMutableAttributedString *marked = [[NSMutableAttributedString alloc] initWithString:preedit];
        for (NSArray<NSNumber *> *span in state[@"spans"]) {
            NSUInteger start = span[0].unsignedIntegerValue;
            NSUInteger end = span[1].unsignedIntegerValue;
            NSInteger style = span[2].boolValue ? NSUnderlineStyleThick : NSUnderlineStyleSingle;
            [marked addAttribute:NSUnderlineStyleAttributeName value:@(style) range:NSMakeRange(start, end - start)];
        }
        _marked = YES;
        [client setMarkedText:marked
               selectionRange:NSMakeRange([state[@"caret"] unsignedIntegerValue], 0)
             replacementRange:NoReplacement()];
    } else if (_marked) {
        _marked = NO;
        [client setMarkedText:@"" selectionRange:NSMakeRange(0, 0) replacementRange:NoReplacement()];
    }
    NSArray<NSString *> *candidates = state[@"candidates"];
    BOOL changed = ![_candidates isEqualToArray:candidates];
    _candidates = candidates;
    _updatingPanel = YES;
    if (_candidates.count) {
        if (changed || ![_panel isVisible]) [_panel updateCandidates];
        if (![_panel isVisible]) [_panel show:kIMKLocateCandidatesBelowHint];
        id selected = state[@"selected"];
        if ([selected isKindOfClass:NSNumber.class] &&
            [selected unsignedIntegerValue] < _candidates.count) {
            NSString *candidate = _candidates[[selected unsignedIntegerValue]];
            [_panel selectCandidateWithIdentifier:[_panel candidateStringIdentifier:candidate]];
        }
    } else {
        [_panel hide];
    }
    _updatingPanel = NO;
}

- (NSArray *)candidates:(id)sender {
    return _candidates ?: @[];
}

- (void)candidateSelected:(NSAttributedString *)candidate {
    if (!_context || !_candidates.count || _updatingPanel) return;
    id client = [self client];
    if (IsSecureEventInputEnabled()) {
        [self discardComposition];
        return;
    }
    NSUInteger row = [_candidates indexOfObject:candidate.string];
    if (row != NSNotFound && sweet_macos_select(_context, row)) [self publishToClient:client];
}

- (NSMenu *)menu {
    NSMenu *menu = [[NSMenu alloc] initWithTitle:@"Sweet IME"];
    NSMenuItem *settings = [menu addItemWithTitle:@"設定…" action:@selector(openSettings:) keyEquivalent:@""];
    settings.target = self;
    return menu;
}

- (void)openSettings:(id)sender {
    NSURL *url = [[NSBundle mainBundle] URLForResource:@"Sweet IME Settings" withExtension:@"app"];
    if (url) {
        [[NSWorkspace sharedWorkspace] openApplicationAtURL:url
                                            configuration:[NSWorkspaceOpenConfiguration configuration]
                                        completionHandler:nil];
    }
}

@end

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        NSBundle *bundle = [NSBundle mainBundle];
        NSString *dictionary = [bundle pathForResource:@"system" ofType:@"dict"];
        if (!dictionary || !sweet_macos_load(dictionary.fileSystemRepresentation)) {
            NSLog(@"Sweet IME could not load system.dict");
            return EXIT_FAILURE;
        }
        if (argc == 2 && strcmp(argv[1], "--smoke-test") == 0) {
            NSString *controllerClass = [bundle objectForInfoDictionaryKey:@"InputMethodServerControllerClass"];
            if (![NSClassFromString(controllerClass) isSubclassOfClass:IMKInputController.class]) {
                return EXIT_FAILURE;
            }
            SweetContext *context = sweet_macos_new();
            if (!context) return EXIT_FAILURE;
            for (const char *key = "kana"; *key; key++) {
                if (!sweet_macos_key(context, (uint16_t)(*key - 'a' + 'A'), (uint32_t)*key, 0)) {
                    sweet_macos_free(context);
                    return EXIT_FAILURE;
                }
            }
            sweet_macos_key(context, 0x20, ' ', 0);
            NSDictionary *state = Snapshot(context);
            BOOL converted = [state[@"candidates"] containsObject:@"仮名"];
            sweet_macos_free(context);
            return converted ? EXIT_SUCCESS : EXIT_FAILURE;
        }
        [NSApplication sharedApplication];
        NSString *connection = [bundle objectForInfoDictionaryKey:@"InputMethodConnectionName"];
        __attribute__((objc_precise_lifetime)) IMKServer *server =
            [[IMKServer alloc] initWithName:connection bundleIdentifier:bundle.bundleIdentifier];
        if (!server) return EXIT_FAILURE;
        [NSApp run];
        return EXIT_SUCCESS;
    }
}
