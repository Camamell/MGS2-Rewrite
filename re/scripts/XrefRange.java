// Print every reference into an address range: from address, containing
// function, reference type.
// Usage: -postScript XrefRange.java <outFile> <startHex> <endHex>
// @category MGS2

import java.io.*;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;

public class XrefRange extends GhidraScript {
	@Override
	protected void run() throws Exception {
		String[] a = getScriptArgs();
		Address s = toAddr(Long.parseLong(a[1], 16));
		Address e = toAddr(Long.parseLong(a[2], 16));
		ReferenceManager rm = currentProgram.getReferenceManager();
		try (PrintWriter w = new PrintWriter(new FileWriter(a[0]))) {
			for (Address t = s; t.compareTo(e) <= 0; t = t.add(1)) {
				for (Reference r : rm.getReferencesTo(t)) {
					Function f = getFunctionContaining(r.getFromAddress());
					w.println(t + " <- " + r.getFromAddress() + " " + (f == null ? "?" : f.getName()) + " " + r.getReferenceType());
				}
			}
		}
	}
}
