/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 */
package info.embryosys.adt.request;

import info.embryosys.adt.core.AdtType;

import java.util.Iterator;
import java.util.List;

/**
 * @author nrdufour
 * 
 */
public final class AdtRequest {

	private final AdtOperation operation;
	private final AdtType type;
	private final List<String> arguments;

	/**
	 * 
	 */
	public AdtRequest(final AdtOperation op, AdtType type,
			final List<String> args) {
		this.operation = op;
		this.type = type;
		this.arguments = args;
	}

	/**
	 * @return the operation
	 */
	public AdtOperation getOperation() {
		return operation;
	}

	/**
	 * @return the type
	 */
	public AdtType getType() {
		return type;
	}

	/**
	 * @return the arguments
	 */
	public Iterator<String> getArguments() {
		return arguments.iterator();
	}

}
