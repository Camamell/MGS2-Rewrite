// Disassemble an address range (creating code where Ghidra left none, e.g.
// the cases of an unrecovered jump table) and print one instruction a line.
// Usage: -postScript DisasmRange.java <outFile> <start> <end> (hex)
// @category MGS2

import java.io.*;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.*;

public class DisasmRange extends GhidraScript {
	@Override
	protected void run() throws Exception {
		String[] args = getScriptArgs();
		Address start = toAddr(Long.parseLong(args[1], 16));
		Address end = toAddr(Long.parseLong(args[2], 16));
		try (PrintWriter w = new PrintWriter(new FileWriter(args[0]))) {
			for (Address a = start; a.compareTo(end) < 0; a = a.add(4)) {
				Instruction i = getInstructionAt(a);
				if (i == null) {
					disassemble(a);
					i = getInstructionAt(a);
				}
				w.println(a + "  " + (i == null ? "??" : i.toString()));
			}
		}
	}
}
