import Foundation

switch ProcessInfo.processInfo.thermalState {
case .nominal: print("nominal")
case .fair: print("fair")
case .serious: print("serious")
case .critical: print("critical")
@unknown default:
  fputs("unknown Foundation thermalState\n", stderr)
  exit(1)
}
