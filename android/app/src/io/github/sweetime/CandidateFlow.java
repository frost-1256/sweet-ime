package io.github.sweetime;

import android.content.Context;
import android.view.View;
import android.view.ViewGroup;

/** Wraps variable-width candidates into rows inside a vertical scroll view. */
final class CandidateFlow extends ViewGroup {
    private final int gap;

    CandidateFlow(Context context, int gap) {
        super(context);
        this.gap = gap;
    }

    @Override
    protected void onMeasure(int widthSpec, int heightSpec) {
        int width = MeasureSpec.getSize(widthSpec);
        setMeasuredDimension(width, arrange(width, true));
    }

    @Override
    protected void onLayout(boolean changed, int left, int top, int right, int bottom) {
        arrange(right - left, false);
    }

    private int arrange(int width, boolean measuring) {
        int x = gap;
        int y = gap;
        int rowHeight = 0;
        int available = Math.max(0, width - 2 * gap);
        for (int i = 0; i < getChildCount(); i++) {
            View child = getChildAt(i);
            if (measuring) {
                child.measure(MeasureSpec.makeMeasureSpec(available, MeasureSpec.AT_MOST),
                        MeasureSpec.makeMeasureSpec(0, MeasureSpec.UNSPECIFIED));
            }
            int w = child.getMeasuredWidth();
            int h = child.getMeasuredHeight();
            if (x > gap && x + w > width - gap) {
                x = gap;
                y += rowHeight + gap;
                rowHeight = 0;
            }
            if (!measuring) child.layout(x, y, x + w, y + h);
            x += w + gap;
            rowHeight = Math.max(rowHeight, h);
        }
        return y + rowHeight + gap;
    }
}
