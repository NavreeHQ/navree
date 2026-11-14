# The protocol models and the view state are read by generated serializers;
# keep them and their companions when an app shrinks its code.
-keepclassmembers class com.navree.protocol.** {
    *** Companion;
}
-keepclasseswithmembers class com.navree.protocol.** {
    kotlinx.serialization.KSerializer serializer(...);
}
-keepclassmembers class com.navree.client.core.** {
    *** Companion;
}
-keepclasseswithmembers class com.navree.client.core.** {
    kotlinx.serialization.KSerializer serializer(...);
}

# The runtime core's library finds its four native methods by the class's and
# the methods' names (Java_com_navree_core_Core_native...): neither may be
# renamed or removed.
-keepclasseswithmembernames,includedescriptorclasses class com.navree.core.Core {
    native <methods>;
}
-keep class com.navree.core.Core {
    native <methods>;
}
