import GenericScreen from "../components/GenericScreen";
import { screenConfig } from "../lib/screenConfigs";

export default function PeriodsRoute() {
  return <GenericScreen config={screenConfig("periods")} />;
}
