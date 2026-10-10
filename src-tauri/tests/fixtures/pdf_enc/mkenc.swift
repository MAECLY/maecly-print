import CoreGraphics
import CoreText
import Foundation
// usage: mkenc <out.pdf> <user-pw or ""> <owner-pw> <allowPrint 0/1> [pages]
let a = CommandLine.arguments
let out = URL(fileURLWithPath: a[1]) as CFURL
let pages = a.count > 5 ? Int(a[5])! : 2
var box = CGRect(x: 0, y: 0, width: 595, height: 842)
var info: [CFString: Any] = [
    kCGPDFContextOwnerPassword: a[3],
    kCGPDFContextAllowsPrinting: a[4] == "1",
    kCGPDFContextTitle: "Pluryprint encrypted fixture",
]
if !a[2].isEmpty { info[kCGPDFContextUserPassword] = a[2] }
let ctx = CGContext(out, mediaBox: &box, info as CFDictionary)!
for i in 0..<pages {
    ctx.beginPDFPage(nil)
    ctx.setFillColor(CGColor(red: 0, green: 0, blue: 0, alpha: 1))
    let attr = NSAttributedString(string: "Pluryprint encrypted test page \(i + 1)",
        attributes: [NSAttributedString.Key(kCTFontAttributeName as String): CTFontCreateWithName("Helvetica" as CFString, 24, nil)])
    let line = CTLineCreateWithAttributedString(attr)
    ctx.textPosition = CGPoint(x: 72, y: 760)
    CTLineDraw(line, ctx)
    ctx.fill(CGRect(x: 72, y: 600, width: 200, height: 100))
    ctx.endPDFPage()
}
ctx.closePDF()
print("ok")
