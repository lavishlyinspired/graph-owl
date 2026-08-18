import * as React from "react";
import { ToastProvider, useToast } from "./toast";

/** Minimal replacement for antd's `<App>`, providing the imperative `message`
 *  API to descendants. The original also exposed `modal` and `notification`;
 *  this console only uses `message`, so only that surface is carried forward. */
export function App({ children }: { children: React.ReactNode }) {
  return <ToastProvider>{children}</ToastProvider>;
}

/** Hook equivalent of `const { message } = App.useApp()` in antd. */
App.useApp = () => {
  const toast = useToast();
  return { message: toast };
};
