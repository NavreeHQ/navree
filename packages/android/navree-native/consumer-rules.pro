# The protocol models are read by generated serializers; keep them and their
# companions when an app shrinks its code.
-keepclassmembers class com.navree.client.protocol.** {
    *** Companion;
}
-keepclasseswithmembers class com.navree.client.protocol.** {
    kotlinx.serialization.KSerializer serializer(...);
}
