/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 */
package info.embryosys.adt.request;

import info.embryosys.adt.core.AdtType;

import java.util.LinkedList;
import java.util.List;

/**
 * @author nrdufour
 * 
 */
public class RequestFactory {

	// FIXME probably need to refactor the exceptions here ...
	public static AdtRequest analyzeString(final String value) {
		if (value == null || value.length() == 0) {
			throw new IllegalArgumentException();
		}

		String[] elements = value.split(" ");

		if (elements.length == 1) {
			throw new IllegalArgumentException("Too few arguments");
		}

		AdtOperation operation = AdtOperation
				.valueOf(elements[0].toUpperCase());
		
		// FIXME need a way to get that ... probably with regexp
		AdtType type = AdtType.NONE;

		List<String> arguments = new LinkedList<String>();
		for(int i = 1; i < elements.length; i++) {
			arguments.add(elements[i]);
		}

		AdtRequest request = new AdtRequest(operation, type, arguments);

		return request;
	}
}
