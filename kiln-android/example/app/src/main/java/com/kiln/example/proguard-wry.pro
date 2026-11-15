# THIS FILE IS AUTO-GENERATED. DO NOT MODIFY!!

# Copyright 2020-2023 Tauri Programme within The Commons Conservancy
# SPDX-License-Identifier: Apache-2.0
# SPDX-License-Identifier: MIT

-keep class com.kiln.example.* {
  native <methods>;
}

-keep class com.kiln.example.WryActivity {
  public <init>(...);

  void setWebView(com.kiln.example.RustWebView);
  java.lang.Class getAppClass(...);
  int getId();
  java.lang.String getVersion();
  int startActivity(...);
}

-keep class com.kiln.example.Ipc {
  public <init>(...);

  @android.webkit.JavascriptInterface public <methods>;
}

-keep class com.kiln.example.RustWebView {
  public <init>(...);

  void loadUrlMainThread(...);
  void loadHTMLMainThread(...);
  void evalScript(...);
  java.lang.String getCookies(java.lang.String);
  void clearAllBrowsingData();
}

-keep class com.kiln.example.RustWebChromeClient,com.kiln.example.RustWebViewClient {
  public <init>(...);
}
