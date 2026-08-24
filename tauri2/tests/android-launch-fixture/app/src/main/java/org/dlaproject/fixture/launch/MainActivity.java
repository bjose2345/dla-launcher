package org.dlaproject.fixture.launch;

import android.app.Activity;
import android.graphics.Color;
import android.os.Bundle;
import android.view.Gravity;
import android.widget.TextView;

public final class MainActivity extends Activity {
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        TextView marker = new TextView(this);
        marker.setBackgroundColor(Color.rgb(20, 18, 25));
        marker.setGravity(Gravity.CENTER);
        marker.setText("DLA Android launch fixture is running");
        marker.setTextColor(Color.rgb(245, 82, 115));
        marker.setTextSize(22);
        setContentView(marker);
    }
}
