import Foundation

let resource = Bundle.main.path(forResource: "message", ofType: "txt")
let message = resource.flatMap { try? String(contentsOfFile: $0, encoding: .utf8) }

print(message?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "Hello from a tinybuild app bundle.")

