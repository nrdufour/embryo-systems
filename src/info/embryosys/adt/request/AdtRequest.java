/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 */
package info.embryosys.adt.request;

import info.embryosys.adt.core.AdtType;

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
	public List<String> getArguments() {
		return arguments;
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see java.lang.Object#toString()
	 */
	@Override
	public String toString() {
		StringBuffer buffer = new StringBuffer();

		buffer.append("Request(op: ");
		buffer.append(this.operation);
		buffer.append(", type: ");
		buffer.append(this.type);
		buffer.append(", arguments: ");
		buffer.append(this.arguments);
		buffer.append(")");

		return buffer.toString();
	}
}
