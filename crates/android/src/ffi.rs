//! JNI entry points and native context ownership.

use std::path::{Path, PathBuf};

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jlong, jstring};

use super::{Context, FIELD, converter, init, pack_zenz, refine, refresh_converter};

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
    _: JClass,
    dict: JString,
    files_dir: JString,
) -> jboolean {
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
