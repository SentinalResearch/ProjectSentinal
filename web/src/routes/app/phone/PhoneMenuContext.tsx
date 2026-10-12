import { createContext, useContext } from "react";

export interface PhoneMenuControls {
  /** Open the full-screen Menu (N1) with search focused; focus returns to `opener` on close. */
  openMenu: (opener?: HTMLElement | null) => void;
}

export const PhoneMenuContext = createContext<PhoneMenuControls>({ openMenu: () => undefined });

export function usePhoneMenu(): PhoneMenuControls {
  return useContext(PhoneMenuContext);
}
