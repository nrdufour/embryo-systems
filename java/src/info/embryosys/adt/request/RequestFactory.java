/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 * 
 * This source code is released under the BSD License.
 */
package info.embryosys.adt.request;

import info.embryosys.adt.core.AdtType;

import java.util.HashMap;
import java.util.LinkedList;
import java.util.List;
import java.util.Map;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * @author nrdufour
 * 
 */
public class RequestFactory {

	private static final String IDENTIFIER_REGEXP = "([a-zA-Z0-9\\-]+)";

	private static final String CLASS_REGEXP = IDENTIFIER_REGEXP;
	private static final String OBJECT_REGEXP = IDENTIFIER_REGEXP + "\\["
			+ IDENTIFIER_REGEXP + "\\]";
	private static final String ATTRIBUTE_REGEXP = IDENTIFIER_REGEXP + "\\."
			+ IDENTIFIER_REGEXP;
	private static final String LINK_REGEXP = IDENTIFIER_REGEXP + "\\-\\>"
			+ IDENTIFIER_REGEXP + "\\-\\>" + IDENTIFIER_REGEXP;

	private static final Map<AdtType, Pattern> TYPE_PATTERNS;

	private static final Map<AdtType, Integer> NAME_INDEX;

	static {
		TYPE_PATTERNS = new HashMap<AdtType, Pattern>();

		TYPE_PATTERNS.put(AdtType.CLASS, Pattern.compile(CLASS_REGEXP));
		TYPE_PATTERNS.put(AdtType.OBJECT, Pattern.compile(OBJECT_REGEXP));
		TYPE_PATTERNS.put(AdtType.ATTRIBUTE, Pattern.compile(ATTRIBUTE_REGEXP));
		TYPE_PATTERNS.put(AdtType.LINK, Pattern.compile(LINK_REGEXP));

		NAME_INDEX = new HashMap<AdtType, Integer>();

		NAME_INDEX.put(AdtType.CLASS, 0);
		NAME_INDEX.put(AdtType.ATTRIBUTE, 1);
		NAME_INDEX.put(AdtType.LINK, 1);
		NAME_INDEX.put(AdtType.OBJECT, 1);
	}

	// FIXME probably need to refactor the exceptions here ...
	public static AdtRequest analyzeString(final String value) {
		if (value == null || value.length() == 0) {
			throw new IllegalArgumentException();
		}

		String[] elements = value.split(" ");

		if (elements.length < 2) {
			throw new IllegalArgumentException("Too few arguments");
		}

		AdtOperation operation = AdtOperation
				.valueOf(elements[0].toUpperCase());

		String argument = elements[1];

		AdtType type = AdtType.NONE;
		List<String> parents = new LinkedList<String>();
		// FIXME bug to fix: the last element should be the name: not the case
		// for link
		for (AdtType possibleType : TYPE_PATTERNS.keySet()) {
			Pattern pattern = TYPE_PATTERNS.get(possibleType);
			Matcher matcher = pattern.matcher(argument);
			if (matcher.matches()) {
				type = possibleType;
				for (int i = 1; i <= matcher.groupCount(); i++) {
					parents.add(matcher.group(i));
				}
			}
		}

		int nameLocation = NAME_INDEX.get(type);
		String name = parents.get(nameLocation);
		parents.remove(nameLocation);

		List<String> arguments = new LinkedList<String>();
		if (elements.length > 2) {
			for (int i = 2; i < elements.length; i++) {
				arguments.add(elements[i]);
			}
		}

		AdtRequest request = new AdtRequest(operation, type, name, parents,
				arguments);

		return request;
	}
}
