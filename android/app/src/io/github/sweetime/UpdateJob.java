package io.github.sweetime;

import android.app.job.JobParameters;
import android.app.job.JobService;

/** The daily update check: a notification when a new release is out. */
public final class UpdateJob extends JobService {
    @Override
    public boolean onStartJob(JobParameters params) {
        new Thread(() -> {
            boolean retry = false;
            try {
                Updater.Release r = Updater.fetchLatest();
                Updater.checked(this);
                if (Updater.offer(this, r)) Updater.notifyAvailable(this, r);
            } catch (Exception e) {
                retry = true;
            }
            jobFinished(params, retry);
        }, "sweet-ime-update-check").start();
        return true;
    }

    @Override
    public boolean onStopJob(JobParameters params) {
        return true;
    }
}
