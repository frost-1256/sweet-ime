//! JNI entry points and native context ownership.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use jni::objects::{GlobalRef, JClass, JString, JValue};
use jni::sys::{jboolean, jint, jlong, jstring};
use jni::{JNIEnv, JavaVM};
use sweet_engine::{UserDictionary, alpha_feedback, user_dict::DEFAULT_POS};

use super::{
    Context, FIELD, converter, feedback_debug, init, pack_zenz, refine, refresh_converter,
};

fn guard<T>(default: T, f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(default)
}

fn to_string(env: &mut JNIEnv, s: &JString) -> String {
    env.get_string(s).map(Into::into).unwrap_or_default()
}

fn to_java(env: &mut JNIEnv, s: Option<String>) -> jstring {
    match s.and_then(|s| env.new_string(s).ok()) {
        Some(s) => s.into_raw(),
        None => std::ptr::null_mut(),
    }
}

unsafe fn ctx<'a>(p: jlong) -> &'a mut Context {
    unsafe { &mut *(p as *mut Context) }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_init(
    mut env: JNIEnv,
    class: JClass,
    dict: JString,
    files_dir: JString,
) -> jboolean {
    register_uploader(&mut env, &class);
    let dict = to_string(&mut env, &dict);
    let files = to_string(&mut env, &files_dir);
    guard(false, || init(PathBuf::from(dict), &files)) as jboolean
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_create(_: JNIEnv, _: JClass) -> jlong {
    guard(0, || Box::into_raw(Box::new(Context::new())) as jlong)
}

/// # Safety
/// `p` comes from `create` and is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_free(_: JNIEnv, _: JClass, p: jlong) {
    if p != 0 {
        drop(unsafe { Box::from_raw(p as *mut Context) });
    }
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_key(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
    code: jint,
) -> jstring {
    let c = unsafe { ctx(p) };
    let s = guard(None, || c.key(code));
    to_java(&mut env, s)
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_symbol(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
    code: jint,
) -> jstring {
    let c = unsafe { ctx(p) };
    let s = guard(None, || c.symbol(char::from_u32(code as u32)?));
    to_java(&mut env, s)
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_select(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
    index: jint,
) -> jstring {
    let c = unsafe { ctx(p) };
    let s = guard(None, || Some(c.select(index.max(0) as usize)));
    to_java(&mut env, s)
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_undoCommit(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
    text: JString,
) -> jstring {
    let text = to_string(&mut env, &text);
    let c = unsafe { ctx(p) };
    let s = guard(None, || c.undo_commit(&text));
    to_java(&mut env, s)
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_commitAll(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
) -> jstring {
    let c = unsafe { ctx(p) };
    let s = guard(None, || Some(c.commit_all()));
    to_java(&mut env, s)
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_reset(_: JNIEnv, _: JClass, p: jlong) {
    let c = unsafe { ctx(p) };
    guard((), || c.reset());
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_reloadOptions(
    _: JNIEnv,
    _: JClass,
    p: jlong,
) {
    let c = unsafe { ctx(p) };
    guard((), || c.reload_options());
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_converting(
    _: JNIEnv,
    _: JClass,
    p: jlong,
) -> jboolean {
    let c = unsafe { ctx(p) };
    guard(false, || c.converting()) as jboolean
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_setLeftContext(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
    text: JString,
) {
    let c = unsafe { ctx(p) };
    let t = to_string(&mut env, &text);
    guard((), || c.set_left_context(&t));
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_liveRequest(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
) -> jstring {
    let c = unsafe { ctx(p) };
    let s = guard(None, || {
        c.live_request().map(|(r, l)| format!("{r}{FIELD}{l}"))
    });
    to_java(&mut env, s)
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_cloudReading(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
) -> jstring {
    to_java(&mut env, guard(None, || unsafe { ctx(p) }.cloud_reading()))
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_applyCloud(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
    reading: JString,
    items: JString,
) -> jstring {
    let reading = to_string(&mut env, &reading);
    let items = to_string(&mut env, &items);
    let snapshot = guard(None, || unsafe { ctx(p) }.apply_cloud(&reading, &items));
    to_java(&mut env, snapshot)
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_liveCommitRequest(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
) -> jstring {
    let c = unsafe { ctx(p) };
    let s = guard(None, || {
        c.live_commit_request()
            .map(|(r, l)| format!("{r}{FIELD}{l}"))
    });
    to_java(&mut env, s)
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_commitRefinedLive(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
    reading: JString,
    left: JString,
    surface: JString,
) -> jstring {
    let c = unsafe { ctx(p) };
    let reading = to_string(&mut env, &reading);
    let left = to_string(&mut env, &left);
    let surface = to_string(&mut env, &surface);
    let s = guard(None, || c.commit_refined_live(&reading, &left, &surface));
    to_java(&mut env, s)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_refine(
    mut env: JNIEnv,
    _: JClass,
    reading: JString,
    left: JString,
) -> jstring {
    let reading = to_string(&mut env, &reading);
    let left = to_string(&mut env, &left);
    let s = guard(None, || refine(&reading, &left));
    to_java(&mut env, s)
}

/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_applyLive(
    mut env: JNIEnv,
    _: JClass,
    p: jlong,
    reading: JString,
    surface: JString,
) -> jstring {
    let c = unsafe { ctx(p) };
    let reading = to_string(&mut env, &reading);
    let surface = to_string(&mut env, &surface);
    let s = guard(None, || c.apply_live(&reading, &surface));
    to_java(&mut env, s)
}

/// Returns null on success, else the error.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_packZenz(
    mut env: JNIEnv,
    _: JClass,
    hf_dir: JString,
    out: JString,
) -> jstring {
    let dir = to_string(&mut env, &hf_dir);
    let out = to_string(&mut env, &out);
    let r = guard(Err("panicked".to_string()), || {
        pack_zenz(Path::new(&dir), Path::new(&out))
    });
    to_java(&mut env, r.err())
}

/// Whether the converter runs with the zenz model now.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_zenzActive(_: JNIEnv, _: JClass) -> jboolean {
    guard(false, || {
        refresh_converter();
        converter().is_some_and(|c| c.has_reranker())
    }) as jboolean
}

/// Marks the field as private (see [`Context::set_private`]).
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_setPrivate(
    _: JNIEnv,
    _: JClass,
    p: jlong,
    on: jboolean,
) {
    let c = unsafe { ctx(p) };
    guard((), || c.set_private(on != 0));
}

/// Lists the reading and surface of each user dictionary entry as TSV.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_userWords(
    mut env: JNIEnv,
    _: JClass,
    path: JString,
) -> jstring {
    let path = to_string(&mut env, &path);
    let words = guard(None, || {
        use std::fmt::Write;

        let mut rows = String::new();
        for word in UserDictionary::try_with_file(path).ok()?.words() {
            let _ = writeln!(rows, "{}\t{}", word.reading, word.surface);
        }
        Some(rows)
    });
    to_java(&mut env, words)
}

/// Registers, edits or deletes a word, returning a Japanese error or null.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_updateUserWord(
    mut env: JNIEnv,
    _: JClass,
    path: JString,
    old_reading: JString,
    old_surface: JString,
    reading: JString,
    surface: JString,
    remove: jboolean,
) -> jstring {
    let path = to_string(&mut env, &path);
    let old_reading = to_string(&mut env, &old_reading);
    let old_surface = to_string(&mut env, &old_surface);
    let reading = to_string(&mut env, &reading);
    let surface = to_string(&mut env, &surface);
    let error = guard(Some("ユーザー辞書を更新できませんでした".into()), || {
        let Ok(mut dict) = UserDictionary::try_with_file(path) else {
            return Some("ユーザー辞書を読み込めませんでした".into());
        };
        if old_reading.is_empty() && remove == 0 {
            return dict
                .add(&reading, &surface, DEFAULT_POS, "")
                .err()
                .map(|error| error.to_string());
        }
        let Some(index) = dict
            .words()
            .iter()
            .position(|word| word.reading == old_reading && word.surface == old_surface)
        else {
            return Some("この単語はユーザー辞書にありません".into());
        };
        if remove != 0 {
            return dict
                .remove(index)
                .err()
                .map(|_| "単語を削除できませんでした".into());
        }
        let original = dict.words()[index].clone();
        dict.replace(index, &reading, &surface, &original.pos, &original.comment)
            .err()
            .map(|error| error.to_string())
    });
    to_java(&mut env, error)
}

/// How many words the user history at `path` holds.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_learnedCount(
    mut env: JNIEnv,
    _: JClass,
    path: JString,
) -> jint {
    let path = to_string(&mut env, &path);
    guard(0, || {
        sweet_engine::UserHistory::with_file(path).len() as jint
    })
}

/// Forgets what the user history at `path` learned in the last `secs`
/// seconds, or everything when `secs` is 0 or less. Returns how many words
/// went, or -1 on failure. A running converter rereads the changed file.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_forgetLearned(
    mut env: JNIEnv,
    _: JClass,
    path: JString,
    secs: jlong,
) -> jint {
    let path = to_string(&mut env, &path);
    guard(-1, || {
        let mut h = sweet_engine::UserHistory::with_file(path);
        let n = h.len();
        let r = if secs > 0 {
            h.forget_recent(std::time::Duration::from_secs(secs as u64))
        } else {
            h.clear().map(|()| n)
        };
        r.map_or(-1, |n| n as jint)
    })
}

/// The JVM and the `Native` class, for uploading Alpha feedback from the
/// engine's worker thread. A thread attached from native code cannot look up
/// app classes by name, so the class is kept from `init`.
static JAVA: OnceLock<(JavaVM, GlobalRef)> = OnceLock::new();

fn register_uploader(env: &mut JNIEnv, class: &JClass) {
    if JAVA.get().is_some() {
        return;
    }
    let (Ok(vm), Ok(class)) = (env.get_java_vm(), env.new_global_ref(class)) else {
        return;
    };
    if JAVA.set((vm, class)).is_ok() {
        alpha_feedback::set_uploader(upload);
    }
}

/// Calls `Native.upload(url, invite, body)` (HttpsURLConnection) on the
/// calling worker thread.
fn upload(url: &str, invite: &str, body: &str) -> std::io::Result<()> {
    let fail = |what: &str| std::io::Error::other(format!("feedback upload: {what}"));
    let (vm, class) = JAVA.get().ok_or_else(|| fail("no JVM"))?;
    let mut env = vm.attach_current_thread().map_err(|_| fail("attach"))?;
    let class: &JClass = class.as_obj().into();
    let (Ok(url), Ok(invite), Ok(body)) = (
        env.new_string(url),
        env.new_string(invite),
        env.new_string(body),
    ) else {
        return Err(fail("strings"));
    };
    let sent = env
        .call_static_method(
            class,
            "upload",
            "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)I",
            &[
                JValue::Object(&url),
                JValue::Object(&invite),
                JValue::Object(&body),
            ],
        )
        .and_then(|v| v.i());
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    match sent {
        Ok(code) if (200..300).contains(&code) => Ok(()),
        Ok(-1) => Err(std::io::Error::other("接続できませんでした")),
        Ok(code) => Err(std::io::Error::other(format!("HTTP {code}"))),
        Err(_) => Err(fail("Java call failed")),
    }
}

/// Whether the field may contribute Alpha feedback (see
/// [`Context::set_feedback_field`]).
///
/// # Safety
/// `p` is a live context.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn Java_io_github_sweetime_Native_setFeedbackField(
    _: JNIEnv,
    _: JClass,
    p: jlong,
    on: jboolean,
) {
    let c = unsafe { ctx(p) };
    guard((), || c.set_feedback_field(on != 0));
}

/// Sends pending feedback without waiting for the batch window.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_feedbackFlush(_: JNIEnv, _: JClass) {
    guard((), alpha_feedback::flush_soon);
}

/// The compiled-in feedback destination, or null when this build cannot send.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_feedbackEndpoint(
    mut env: JNIEnv,
    _: JClass,
) -> jstring {
    let url = guard(None, || alpha_feedback::endpoint().map(str::to_owned));
    to_java(&mut env, url)
}

/// Whether the consent file at `path` holds a valid consent.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_feedbackConsented(
    mut env: JNIEnv,
    _: JClass,
    path: JString,
) -> jboolean {
    let path = to_string(&mut env, &path);
    guard(false, || {
        alpha_feedback::consent_at(Path::new(&path)).is_some()
    }) as jboolean
}

/// Records consent with `invite` at `path`. Returns whether it was saved.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_feedbackEnable(
    mut env: JNIEnv,
    _: JClass,
    path: JString,
    invite: JString,
) -> jboolean {
    let path = to_string(&mut env, &path);
    let invite = to_string(&mut env, &invite);
    guard(false, || {
        alpha_feedback::enable_at(Path::new(&path), invite.trim()).is_ok()
    }) as jboolean
}

/// Withdraws consent. Returns whether no consent remains.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_feedbackDisable(
    mut env: JNIEnv,
    _: JClass,
    path: JString,
) -> jboolean {
    let path = to_string(&mut env, &path);
    guard(false, || {
        alpha_feedback::disable_at(Path::new(&path)).is_ok()
    }) as jboolean
}

/// The Alpha feedback state for the debug menu (no conversion text).
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_feedbackDebug(
    mut env: JNIEnv,
    _: JClass,
    consent_file: JString,
) -> jstring {
    let path = to_string(&mut env, &consent_file);
    let report = guard(None, || Some(feedback_debug(Path::new(&path))));
    to_java(&mut env, report)
}

/// Sends a fixed test record with the consent at `path`; false without consent.
#[unsafe(no_mangle)]
pub extern "system" fn Java_io_github_sweetime_Native_feedbackSendTest(
    mut env: JNIEnv,
    _: JClass,
    path: JString,
) -> jboolean {
    let path = to_string(&mut env, &path);
    guard(false, || alpha_feedback::send_test_at(Path::new(&path))) as jboolean
}
