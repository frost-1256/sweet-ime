#import <AppKit/AppKit.h>
#import <Carbon/Carbon.h>
#import <InputMethodKit/InputMethodKit.h>
#import <IOKit/hidsystem/IOLLEvent.h>
#import <dispatch/dispatch.h>

#include <stdlib.h>
#include <stdio.h>
#include <string.h>

#include "bridge.h"
#include "candidate_panel.h"

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

static BOOL HandleModifier(SweetContext *context, NSEvent *event) {
    uint16_t vk;
    NSUInteger side;
    NSUInteger family;
    switch (event.keyCode) {
        case kVK_Shift:        vk = 0xA0; side = NX_DEVICELSHIFTKEYMASK; family = NSEventModifierFlagShift; break;
        case kVK_RightShift:   vk = 0xA1; side = NX_DEVICERSHIFTKEYMASK; family = NSEventModifierFlagShift; break;
        case kVK_Control:      vk = 0xA2; side = NX_DEVICELCTLKEYMASK; family = NSEventModifierFlagControl; break;
        case kVK_RightControl: vk = 0xA3; side = NX_DEVICERCTLKEYMASK; family = NSEventModifierFlagControl; break;
        case kVK_Option:       vk = 0xA4; side = NX_DEVICELALTKEYMASK; family = NSEventModifierFlagOption; break;
        case kVK_RightOption:  vk = 0xA5; side = NX_DEVICERALTKEYMASK; family = NSEventModifierFlagOption; break;
        default: return sweet_macos_modifier(context, 0, true, false);
    }
    const NSUInteger modifiers = NSEventModifierFlagShift | NSEventModifierFlagControl |
        NSEventModifierFlagOption | NSEventModifierFlagCommand | NSEventModifierFlagFunction;
    const NSUInteger sides = NX_DEVICELSHIFTKEYMASK | NX_DEVICERSHIFTKEYMASK |
        NX_DEVICELCTLKEYMASK | NX_DEVICERCTLKEYMASK | NX_DEVICELALTKEYMASK | NX_DEVICERALTKEYMASK;
    const NSUInteger flags = event.modifierFlags;
    const BOOL alone = (flags & modifiers & ~family) == 0 && (flags & sides & ~side) == 0;
    return sweet_macos_modifier(context, vk, (flags & side) != 0, alone);
}

@interface SweetInputController : IMKInputController
- (void)selectCandidateRow:(NSUInteger)row;
@end

@implementation SweetInputController {
    SweetContext *_context;
    SweetCandidatePanel *_panel;
    BOOL _marked;
    BOOL _pollScheduled;
}

- (instancetype)initWithServer:(IMKServer *)server delegate:(id)delegate client:(id)client {
    self = [super initWithServer:server delegate:delegate client:client];
    if (self) {
        _context = sweet_macos_new();
    }
    return self;
}

- (void)dealloc {
    [_panel hide];
    if (_context) sweet_macos_free(_context);
}

- (NSUInteger)recognizedEvents:(id)sender {
    return NSEventMaskKeyDown | NSEventMaskFlagsChanged;
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
    if (!_context) return NO;
    if (IsSecureEventInputEnabled()) {
        [self discardComposition];
        return NO;
    }
    if (event.type == NSEventTypeFlagsChanged) {
        if (HandleModifier(_context, event)) {
            [self publishToClient:sender];
            [self scheduleLiveUpdate];
        }
        return NO;
    }
    if (event.type != NSEventTypeKeyDown) return NO;
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
    NSUInteger anchorIndex = 0;
    if (commit.length) {
        _marked = NO;
        [client insertText:commit replacementRange:NoReplacement()];
    }
    if (preedit.length) {
        NSMutableAttributedString *marked = [[NSMutableAttributedString alloc] initWithString:preedit];
        for (NSArray<NSNumber *> *span in state[@"spans"]) {
            NSUInteger start = span[0].unsignedIntegerValue;
            NSUInteger end = span[1].unsignedIntegerValue;
            BOOL focused = span[2].boolValue;
            if (focused) anchorIndex = start;
            NSInteger style = focused ? NSUnderlineStyleThick : NSUnderlineStyleSingle;
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
    if (!candidates.count) {
        [_panel hide];
        return;
    }

    NSRect anchor = NSZeroRect;
    [client attributesForCharacterIndex:anchorIndex lineHeightRectangle:&anchor];
    if (NSWidth(anchor) <= 0 && NSHeight(anchor) <= 0 && anchorIndex != 0) {
        [client attributesForCharacterIndex:0 lineHeightRectangle:&anchor];
    }
    if (NSWidth(anchor) <= 0 && NSHeight(anchor) <= 0) {
        [_panel hide];
        return;
    }
    if (!_panel) {
        __weak SweetInputController *controller = self;
        _panel = [[SweetCandidatePanel alloc] initWithSelection:^(NSUInteger row) {
            [controller selectCandidateRow:row];
        }];
    }
    id selected = state[@"selected"];
    NSUInteger row = [selected isKindOfClass:NSNumber.class] ? [selected unsignedIntegerValue] : NSNotFound;
    [_panel showCandidates:candidates selectedRow:row anchor:anchor];
}

- (void)selectCandidateRow:(NSUInteger)row {
    if (!_context) return;
    id client = [self client];
    if (!client || IsSecureEventInputEnabled()) {
        [self discardComposition];
        return;
    }
    if (sweet_macos_select(_context, row)) [self publishToClient:client];
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

static NSString *InputSourceID(NSBundle *bundle) {
    NSString *bundleID = bundle.bundleIdentifier;
    NSString *connection = [bundle objectForInfoDictionaryKey:@"InputMethodConnectionName"];
    NSDictionary *modes = [bundle objectForInfoDictionaryKey:@"ComponentInputModeDict"];
    NSArray<NSString *> *visibleModes = modes[@"tsVisibleInputModeOrderedArrayKey"];
    NSString *sourceID = visibleModes.firstObject;
    NSDictionary *mode = modes[@"tsInputModeListKey"][sourceID ?: @""];
    if ([bundleID containsString:@".inputmethod."] &&
        [connection isEqualToString:[bundleID stringByAppendingString:@"_Connection"]] &&
        [sourceID hasPrefix:[bundleID stringByAppendingString:@"."]] &&
        [mode[@"TISInputSourceID"] isEqualToString:sourceID] &&
        [mode[@"TISIntendedLanguage"] isEqualToString:@"ja"] &&
        [mode[@"tsInputModeScriptKey"] isEqualToString:@"smJapanese"] &&
        [mode[@"tsInputModeIsVisibleKey"] boolValue]) {
        return sourceID;
    }
    return nil;
}

static int CheckInputSource(NSBundle *bundle) {
    NSString *sourceID = InputSourceID(bundle);
    if (!sourceID) {
        fputs("Sweet IME has invalid input source metadata.\n", stderr);
        return EXIT_FAILURE;
    }
    NSDictionary *filter = @{(__bridge NSString *)kTISPropertyInputSourceID: sourceID};
    NSArray *sources = CFBridgingRelease(TISCreateInputSourceList((__bridge CFDictionaryRef)filter, true));
    if (!sources.count) {
        fprintf(stderr, "%s is missing from this login session.\n", sourceID.UTF8String);
        return EXIT_FAILURE;
    }
    NSArray *enabled = CFBridgingRelease(TISCreateInputSourceList((__bridge CFDictionaryRef)filter, false));
    printf("%s is %s.\n", sourceID.UTF8String, enabled.count ? "registered and enabled" : "registered but disabled");
    return EXIT_SUCCESS;
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        NSBundle *bundle = [NSBundle mainBundle];
        if (argc == 2 && strcmp(argv[1], "--check-input-source") == 0) {
            return CheckInputSource(bundle);
        }
        if (argc == 3 && strcmp(argv[1], "--candidate-panel-preview") == 0) {
            return SweetCandidatePanelPreview([NSString stringWithUTF8String:argv[2]])
                ? EXIT_SUCCESS : EXIT_FAILURE;
        }
        NSString *dictionary = [bundle pathForResource:@"system" ofType:@"dict"];
        if (!dictionary || !sweet_macos_load(dictionary.fileSystemRepresentation)) {
            NSLog(@"Sweet IME could not load system.dict");
            return EXIT_FAILURE;
        }
        if (argc == 2 && strcmp(argv[1], "--smoke-test") == 0) {
            NSString *controllerClass = [bundle objectForInfoDictionaryKey:@"InputMethodServerControllerClass"];
            if (!InputSourceID(bundle) ||
                ![NSClassFromString(controllerClass) isSubclassOfClass:IMKInputController.class]) {
                fputs("Sweet IME has invalid input source metadata or controller class.\n", stderr);
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
            NSArray<NSString *> *candidates = state[@"candidates"];
            BOOL converted = [candidates containsObject:@"仮名"] &&
                             [state[@"selected"] isEqual:@0];
            for (NSUInteger row = 1; converted && row <= 2; row++) {
                converted = row < candidates.count && sweet_macos_key(context, 0x09, 0, 0);
                if (!converted) break;
                NSDictionary *next = Snapshot(context);
                converted = [next[@"candidates"] isEqual:candidates] &&
                            [next[@"selected"] isEqual:@(row)] &&
                            [next[@"preedit"] isEqual:candidates[row]];
            }
            sweet_macos_free(context);

            NSRect screen = NSMakeRect(100, 50, 800, 600);
            NSRect anchor = NSMakeRect(850, 64, 1, 20);
            NSRect small = SweetCandidatePanelFrame(2, 190, anchor, screen);
            NSRect large = SweetCandidatePanelFrame(8, 260, anchor, screen);
            BOOL fitted = NSHeight(small) < NSHeight(large) &&
                          NSWidth(small) < NSWidth(large) &&
                          NSMinY(small) >= NSMaxY(anchor) &&
                          NSContainsRect(screen, small) && NSContainsRect(screen, large);
            if (!converted || !fitted) fputs("Candidate selection or geometry smoke test failed.\n", stderr);
            return converted && fitted ? EXIT_SUCCESS : EXIT_FAILURE;
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
