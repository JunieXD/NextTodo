package cn.juniexd.nexttodo

import android.os.Bundle
import com.getcapacitor.BridgeActivity

class MainActivity : BridgeActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        registerPlugin(NextTodoPlugin::class.java)
        WebUpdate.start(this)
        super.onCreate(savedInstanceState)
    }
}
