# JNA + UniFFI generated bindings are reached reflectively.
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class ro.dragoscatalin.scrin.ffi.** { *; }
-dontwarn java.awt.**
