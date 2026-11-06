import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import "../styles.css"
import { TabsScreen } from "../screens/tabs"

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <TabsScreen />
  </StrictMode>
)
