/* Link only into the Mavericks build. */
#include <errno.h>
#include <mach/mach_time.h>
#include <stdlib.h>
#include <sys/time.h>
#include <time.h>

/* CommonCrypto's random API is unavailable in the 10.9 SDK. */
int CCRandomGenerateBytes(void *bytes, size_t count) {
    if (count != 0) arc4random_buf(bytes, count);
    return 0;
}

/* Darwin clock IDs used by Rust's SystemTime and Instant. */
int clock_gettime(int clock, struct timespec *out) {
    switch (clock) {
    case 0: { /* CLOCK_REALTIME */
        struct timeval now;
        if (gettimeofday(&now, NULL) != 0) return -1;
        out->tv_sec = now.tv_sec;
        out->tv_nsec = now.tv_usec * 1000L;
        return 0;
    }
    case 8: { /* CLOCK_UPTIME_RAW */
        mach_timebase_info_data_t scale;
        if (mach_timebase_info(&scale) != KERN_SUCCESS || !scale.denom) {
            errno = EIO;
            return -1;
        }
        unsigned __int128 nanos = (unsigned __int128)mach_absolute_time() * scale.numer / scale.denom;
        out->tv_sec = (time_t)(nanos / 1000000000);
        out->tv_nsec = (long)(nanos % 1000000000);
        return 0;
    }
    default:
        errno = EINVAL;
        return -1;
    }
}
