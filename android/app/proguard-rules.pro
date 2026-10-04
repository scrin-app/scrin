# kotlinx.serialization (navigation3 route keys)
-keepattributes *Annotation*, InnerClasses
-keepclassmembers class ro.dragoscatalin.scrin.** { *** Companion; }
-keepclasseswithmembers class ro.dragoscatalin.scrin.** { kotlinx.serialization.KSerializer serializer(...); }
# Release builds never log: strip verbose/debug/info calls.
-assumenosideeffects class android.util.Log {
    public static int v(...);
    public static int d(...);
    public static int i(...);
}
