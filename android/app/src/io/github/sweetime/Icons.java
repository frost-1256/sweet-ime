package io.github.sweetime;

import android.graphics.Canvas;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.RectF;

/**
 * Line icons for the keys, drawn on a 24-unit grid in the style of
 * outlined Material symbols.
 */
final class Icons {
    static final int SHIFT = 1;
    static final int SHIFT_ON = 2;
    static final int CAPS_LOCK = 3;
    static final int BACKSPACE = 4;
    static final int RETURN = 5;
    static final int SEARCH = 6;
    static final int SEND = 7;
    static final int FORWARD = 8;
    static final int BACK = 9;
    static final int DONE = 10;
    static final int LEFT = 11;
    static final int RIGHT = 12;
    static final int GLOBE = 13;
    static final int SETTINGS = 14;

    private final Paint stroke = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Paint fill = new Paint(Paint.ANTI_ALIAS_FLAG);
    private final Path path = new Path();

    Icons() {
        stroke.setStyle(Paint.Style.STROKE);
        stroke.setStrokeCap(Paint.Cap.ROUND);
        stroke.setStrokeJoin(Paint.Join.ROUND);
        fill.setStyle(Paint.Style.FILL);
    }

    /** Draws `icon` `size` pixels wide, centered in `r`. */
    void draw(Canvas c, int icon, RectF r, float size, int color) {
        float u = size / 24f;
        c.save();
        c.translate(r.centerX() - size / 2, r.centerY() - size / 2);
        c.scale(u, u);
        stroke.setColor(color);
        stroke.setStrokeWidth(1.9f);
        fill.setColor(color);
        path.reset();
        switch (icon) {
            case SHIFT:
            case SHIFT_ON:
            case CAPS_LOCK: {
                float bottom = icon == CAPS_LOCK ? 16.5f : 19.5f;
                path.moveTo(12, 3.5f);
                path.lineTo(20, 12);
                path.lineTo(15.5f, 12);
                path.lineTo(15.5f, bottom);
                path.lineTo(8.5f, bottom);
                path.lineTo(8.5f, 12);
                path.lineTo(4, 12);
                path.close();
                if (icon == SHIFT) {
                    c.drawPath(path, stroke);
                } else {
                    c.drawPath(path, fill);
                    c.drawPath(path, stroke);
                }
                if (icon == CAPS_LOCK) c.drawLine(8.5f, 20, 15.5f, 20, stroke);
                break;
            }
            case BACKSPACE:
                path.moveTo(8.5f, 5.5f);
                path.lineTo(19, 5.5f);
                path.quadTo(21.5f, 5.5f, 21.5f, 8);
                path.lineTo(21.5f, 16);
                path.quadTo(21.5f, 18.5f, 19, 18.5f);
                path.lineTo(8.5f, 18.5f);
                path.lineTo(2.5f, 12);
                path.close();
                c.drawPath(path, stroke);
                c.drawLine(11.5f, 9, 17.5f, 15, stroke);
                c.drawLine(17.5f, 9, 11.5f, 15, stroke);
                break;
            case RETURN:
                path.moveTo(19, 5);
                path.lineTo(19, 10.5f);
                path.quadTo(19, 13.5f, 16, 13.5f);
                path.lineTo(5, 13.5f);
                c.drawPath(path, stroke);
                arrowHead(c, 5, 13.5f, -1);
                break;
            case SEARCH:
                c.drawCircle(10, 10, 6, stroke);
                c.drawLine(14.5f, 14.5f, 20, 20, stroke);
                break;
            case SEND:
                path.moveTo(3.5f, 4.5f);
                path.lineTo(21, 12);
                path.lineTo(3.5f, 19.5f);
                path.lineTo(6, 12);
                path.close();
                c.drawPath(path, stroke);
                c.drawLine(6, 12, 12, 12, stroke);
                break;
            case FORWARD:
                c.drawLine(4, 12, 19.5f, 12, stroke);
                arrowHead(c, 19.5f, 12, 1);
                break;
            case BACK:
                c.drawLine(20, 12, 4.5f, 12, stroke);
                arrowHead(c, 4.5f, 12, -1);
                break;
            case DONE:
                path.moveTo(4.5f, 12.5f);
                path.lineTo(9.5f, 17.5f);
                path.lineTo(19.5f, 7);
                c.drawPath(path, stroke);
                break;
            case LEFT:
                path.moveTo(14.5f, 6);
                path.lineTo(8.5f, 12);
                path.lineTo(14.5f, 18);
                stroke.setStrokeWidth(2.2f);
                c.drawPath(path, stroke);
                break;
            case RIGHT:
                path.moveTo(9.5f, 6);
                path.lineTo(15.5f, 12);
                path.lineTo(9.5f, 18);
                stroke.setStrokeWidth(2.2f);
                c.drawPath(path, stroke);
                break;
            case GLOBE:
                c.drawCircle(12, 12, 9, stroke);
                c.drawOval(8, 3, 16, 21, stroke);
                c.drawLine(3, 12, 21, 12, stroke);
                stroke.setStrokeWidth(1.5f);
                c.drawLine(4.3f, 7.5f, 19.7f, 7.5f, stroke);
                c.drawLine(4.3f, 16.5f, 19.7f, 16.5f, stroke);
                break;
            case SETTINGS:
                gear(c);
                break;
            default:
                break;
        }
        c.restore();
    }

    /** An open arrow head at (x, y) pointing right (dir 1) or left (-1). */
    private void arrowHead(Canvas c, float x, float y, int dir) {
        Path p = new Path();
        p.moveTo(x - 5 * dir, y - 5);
        p.lineTo(x, y);
        p.lineTo(x - 5 * dir, y + 5);
        c.drawPath(p, stroke);
    }

    /** A gear with eight teeth and a hole. */
    private void gear(Canvas c) {
        float outer = 9.5f;
        float inner = 7.2f;
        for (int i = 0; i < 8; i++) {
            double t = Math.toRadians(i * 45);
            double half = Math.toRadians(10);
            double rise = Math.toRadians(16);
            point(i == 0, t - rise, inner);
            point(false, t - half, outer);
            point(false, t + half, outer);
            point(false, t + rise, inner);
        }
        path.close();
        c.drawPath(path, stroke);
        c.drawCircle(12, 12, 3, stroke);
    }

    private void point(boolean first, double angle, float radius) {
        float x = 12 + (float) (Math.cos(angle) * radius);
        float y = 12 + (float) (Math.sin(angle) * radius);
        if (first) path.moveTo(x, y);
        else path.lineTo(x, y);
    }
}
